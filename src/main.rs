
use std::io::{self, BufRead};
use std::collections::{HashMap, HashSet};
use clap::Parser as ClapParser;
use thiserror::Error;

// CLI arguments definition
#[derive(ClapParser, Debug)]
#[clap(author, version, about = "A high-performance log viewing and processing tool")]
struct Args {
    /// Comma-separated list of keys to display
    #[clap(short = 'k', long = "keys", value_delimiter = ',')]
    keys: Option<Vec<String>>,

    /// When specified, only these keys will be included in the output
    #[clap(short = 'i', long = "include-only", action)]
    include_only: bool,
}

// Core data types
#[derive(Debug, Clone)]
enum Value {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Array(Vec<Value>),
    Object(HashMap<String, Value>),
}

impl From<serde_json::Value> for Value {
    fn from(json: serde_json::Value) -> Self {
        match json {
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Number(n) => {
                if let Some(n) = n.as_f64() {
                    Value::Number(n)
                } else {
                    // Handle integers that don't fit in f64
                    Value::String(n.to_string())
                }
            },
            serde_json::Value::Bool(b) => Value::Boolean(b),
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Array(arr) => {
                Value::Array(arr.into_iter().map(Value::from).collect())
            },
            serde_json::Value::Object(obj) => {
                Value::Object(
                    obj.into_iter()
                        .map(|(k, v)| (k, Value::from(v)))
                        .collect()
                )
            },
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::String(s) => write!(f, "\"{}\"", s.replace("\"", "\\\"")),
            Value::Number(n) => write!(f, "{}", n),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Null => write!(f, "null"),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, v) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", v)?;
                }
                write!(f, "]")
            },
            Value::Object(obj) => {
                write!(f, "{{")?;
                for (i, (k, v)) in obj.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\": {}", k, v)?;
                }
                write!(f, "}}")
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Event {
    fields: HashMap<String, Value>,
}

impl Event {
    fn new() -> Self {
        Event {
            fields: HashMap::new(),
        }
    }

    fn insert(&mut self, key: String, value: Value) {
        self.fields.insert(key, value);
    }

    fn fields(&self) -> &HashMap<String, Value> {
        &self.fields
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }
}

// Error handling
#[derive(Debug, Error)]
enum KeloraError {
    #[error("Failed to parse: {0}")]
    ParseError(String),
    
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

// Parser trait and implementation
trait LogParser {
    fn parse(&self, line: &str) -> Result<Event, KeloraError>;
}

struct JsonParser;

impl LogParser for JsonParser {
    fn parse(&self, line: &str) -> Result<Event, KeloraError> {
        let json_value: serde_json::Value = serde_json::from_str(line)
            .map_err(|e| KeloraError::ParseError(format!("JSON parse error: {}", e)))?;
        
        let mut event = Event::new();
        
        if let serde_json::Value::Object(obj) = json_value {
            for (key, value) in obj {
                event.insert(key, Value::from(value));
            }
        } else {
            return Err(KeloraError::ParseError("Root JSON value is not an object".to_string()));
        }
        
        Ok(event)
    }
}

// Transformer trait and implementation
trait Transformer {
    fn transform(&mut self, event: Event) -> Vec<Event>;
    fn flush(&mut self) -> Vec<Event> { Vec::new() }
}

struct FieldSelector {
    fields: HashSet<String>,
    include_only: bool,
}

impl FieldSelector {
    fn new(fields: HashSet<String>, include_only: bool) -> Self {
        FieldSelector { 
            fields,
            include_only,
        }
    }
}

impl Transformer for FieldSelector {
    fn transform(&mut self, event: Event) -> Vec<Event> {
        if self.fields.is_empty() {
            return vec![event];
        }

        let mut new_event = Event::new();
        
        if self.include_only {
            // Include only mode: only keep specified fields
            for field in &self.fields {
                if let Some(value) = event.get(field) {
                    new_event.insert(field.clone(), value.clone());
                }
            }
        } else {
            // Default mode: include all fields
            for (key, value) in event.fields() {
                new_event.insert(key.clone(), value.clone());
            }
        }
        
        vec![new_event]
    }
}

// Formatter trait and implementation
trait Formatter {
    fn format(&self, event: &Event) -> String;
}

struct LogfmtFormatter;

impl Formatter for LogfmtFormatter {
    fn format(&self, event: &Event) -> String {
        let mut parts = Vec::new();
        
        // Sort keys for consistent output
        let mut keys: Vec<&String> = event.fields().keys().collect();
        keys.sort();
        
        for key in keys {
            if let Some(value) = event.get(key) {
                let formatted_value = match value {
                    Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
                    Value::Number(n) => n.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    Value::Null => "null".to_string(),
                    Value::Array(_) | Value::Object(_) => format!("\"{}\"", value.to_string().replace("\"", "\\\"")),
                };
                parts.push(format!("{}={}", key, formatted_value));
            }
        }
        
        parts.join(" ")
    }
}

// Pipeline trait and implementation
trait Pipeline {
    fn process(&mut self, event: Event) -> Vec<Event>;
    fn flush(&mut self) -> Vec<Event>;
}

struct ProcessingPipeline {
    transformers: Vec<Box<dyn Transformer>>,
}

impl ProcessingPipeline {
    fn new() -> Self {
        ProcessingPipeline {
            transformers: Vec::new(),
        }
    }
    
    fn add_transformer(&mut self, transformer: Box<dyn Transformer>) {
        self.transformers.push(transformer);
    }
}

impl Pipeline for ProcessingPipeline {
    fn process(&mut self, event: Event) -> Vec<Event> {
        let mut events = vec![event];
        
        // Apply all transformers in sequence
        for transformer in &mut self.transformers {
            events = events.into_iter()
                .flat_map(|e| transformer.transform(e))
                .collect();
        }
        
        events
    }
    
    fn flush(&mut self) -> Vec<Event> {
        let mut results = Vec::new();
        for transformer in &mut self.transformers {
            results.extend(transformer.flush());
        }
        
        results
    }
}

fn main() -> Result<(), KeloraError> {
    // Parse command line arguments
    let args = Args::parse();  // This works because ClapParser is in scope
    
    // Set up the parser
    let parser = JsonParser;
    
    // Set up the formatter
    let formatter = LogfmtFormatter;
    
    // Create processing pipeline
    let mut pipeline = ProcessingPipeline::new();
    
    // Add field selector if keys were specified
    if let Some(keys) = args.keys {
        let fields: HashSet<String> = keys.into_iter().collect();
        pipeline.add_transformer(Box::new(FieldSelector::new(fields, args.include_only)));
    }
    
    // Process input line by line
    let stdin = io::stdin();
    for line_result in stdin.lock().lines() {
        let line = line_result?;
        
        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }
        
        // Parse the line
        match parser.parse(&line) {
            Ok(event) => {
                // Process the event through the pipeline
                let processed_events = pipeline.process(event);
                
                // Format and output each resulting event
                for event in processed_events {
                    println!("{}", formatter.format(&event));
                }
            },
            Err(e) => {
                eprintln!("Error processing line: {}", e);
                // Continue processing despite errors
            },
        }
    }
    
    // Flush the pipeline and output any remaining events
    for event in pipeline.flush() {
        println!("{}", formatter.format(&event));
    }
    
    Ok(())
}