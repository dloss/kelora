use crate::event::{flatten_dynamic, Event, FlattenStyle};
use crate::pipeline;

use rhai::Dynamic;
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// Global header tracking registry for CSV formatters in parallel mode
/// Key format: "{delimiter}_{keys_hash}" for uniqueness across different CSV configurations
static CSV_FORMATTER_HEADER_REGISTRY: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Check if a CSV value needs quoting
pub(crate) fn needs_csv_quoting(value: &str, delimiter: char) -> bool {
    value.is_empty()
        || value.contains(delimiter)
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r')
        || value.starts_with(' ')
        || value.ends_with(' ')
}

/// Escape CSV value with proper quoting
pub(crate) fn escape_csv_value(value: &str, delimiter: char) -> String {
    if needs_csv_quoting(value, delimiter) {
        let escaped = value.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    }
}

// CSV formatter - outputs CSV format with required field order
pub struct CsvFormatter {
    delimiter: char,
    keys: Vec<String>,
    include_header: bool,
    formatter_key: String,
    worker_mode: bool, // If true, never write headers (for parallel workers)
}

impl CsvFormatter {
    pub fn new(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: true,
            formatter_key,
            worker_mode: false,
        }
    }

    pub fn new_tsv(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: true,
            formatter_key,
            worker_mode: false,
        }
    }

    pub fn new_csv_no_header(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_noheader_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: false,
        }
    }

    pub fn new_tsv_no_header(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_noheader_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: false,
        }
    }

    /// Create worker-mode variants that never write headers
    pub fn new_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: false, // Workers never write headers
            formatter_key,
            worker_mode: true,
        }
    }

    pub fn new_tsv_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: false, // Workers never write headers
            formatter_key,
            worker_mode: true,
        }
    }

    pub fn new_csv_no_header_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!(",_noheader_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: ',',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: true,
        }
    }

    pub fn new_tsv_no_header_worker(keys: Vec<String>) -> Self {
        let formatter_key = format!("\t_noheader_worker_{}", Self::keys_hash(&keys));
        Self {
            delimiter: '\t',
            keys,
            include_header: false,
            formatter_key,
            worker_mode: true,
        }
    }

    /// Create a simple hash of the keys for uniqueness
    fn keys_hash(keys: &[String]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        keys.hash(&mut hasher);
        hasher.finish()
    }

    /// Mark header as written globally for this formatter configuration
    /// Returns true if this call was the first to mark it (header should be written)
    fn mark_header_written_globally(&self) -> bool {
        let mut registry = CSV_FORMATTER_HEADER_REGISTRY.lock().unwrap();
        if registry.get(&self.formatter_key).copied().unwrap_or(false) {
            // Already marked by another thread
            false
        } else {
            // This is the first thread to mark it
            registry.insert(self.formatter_key.clone(), true);
            true
        }
    }

    /// Format the header row
    pub fn format_header(&self) -> String {
        self.keys
            .iter()
            .map(|key| escape_csv_value(key, self.delimiter))
            .collect::<Vec<_>>()
            .join(&self.delimiter.to_string())
    }

    /// Format a data row
    fn format_data_row(&self, event: &Event) -> String {
        self.keys
            .iter()
            .map(|key| {
                if let Some(value) = event.fields.get(key) {
                    let string_value = self.format_csv_value(value);
                    escape_csv_value(&string_value, self.delimiter)
                } else {
                    String::new() // Empty field for missing values
                }
            })
            .collect::<Vec<_>>()
            .join(&self.delimiter.to_string())
    }

    /// Format a Dynamic value for CSV output, flattening nested structures
    fn format_csv_value(&self, value: &Dynamic) -> String {
        // Check if this is a complex nested structure
        if value.clone().try_cast::<rhai::Map>().is_some()
            || value.clone().try_cast::<rhai::Array>().is_some()
        {
            // Flatten nested structures using underscore style for CSV safety
            let flattened = flatten_dynamic(value, FlattenStyle::Underscore, 0);

            // flatten_dynamic always yields at least one entry; an empty
            // map/array collapses to a single UNIT placeholder, rendered as an
            // empty cell. Every other case - including a single real key - must
            // keep its "key:value" shape so nested keys/indices are not lost.
            if flattened.is_empty()
                || (flattened.len() == 1 && flattened.values().next().unwrap().is_unit())
            {
                String::new()
            } else {
                // Compact representation: "key1:val1,key2:val2".
                flattened
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k, v))
                    .collect::<Vec<_>>()
                    .join(",")
            }
        } else if let Some(s) = crate::rhai_functions::datetime::render_custom_scalar(value) {
            s
        } else {
            // Simple scalar value
            value.to_string()
        }
    }
}

impl pipeline::Formatter for CsvFormatter {
    fn format(&self, event: &Event) -> String {
        let mut output = String::new();

        // Write header row if needed (thread-safe, once only across all workers)
        // Workers in parallel mode never write headers
        if !self.worker_mode && self.include_header && self.mark_header_written_globally() {
            output.push_str(&self.format_header());
            output.push('\n');
        }

        // Write data row
        output.push_str(&self.format_data_row(event));
        output
    }
}
