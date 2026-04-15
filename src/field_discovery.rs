//! Field discovery: schema profiling for log streams.
//!
//! Scans events and reports per-field statistics: occurrence counts,
//! type distributions, cardinality estimates, and sample values.
//!
//! Uses a hybrid cardinality strategy:
//!   - Small sets (≤ threshold): exact `HashSet` tracking
//!   - Large sets: graduated to HyperLogLog estimation

use hyperloglog::HyperLogLog;
use indexmap::IndexMap;
use rhai::Dynamic;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::{Hash, Hasher};
use unicode_width::UnicodeWidthStr;

/// Threshold at which we graduate from exact tracking to HLL estimation.
const EXACT_CARDINALITY_THRESHOLD: usize = 256;

/// Maximum number of sample values to keep per field.
const MAX_SAMPLES: usize = 8;

/// Default table width for `--discover` when output is redirected (not a TTY)
/// and no `COLUMNS` override is set. Chosen wide enough that the examples
/// column has room to breathe in files and pipes.
const REDIRECTED_TABLE_WIDTH: usize = 200;

/// Maximum number of distinct fields to track (memory safety).
const MAX_TRACKED_FIELDS: usize = 1_000;

/// Default maximum depth for flattening nested maps and arrays into dotted
/// keys. Depth counts descents from the event root: `a.b.c` is depth 3.
/// Override with `--discover-depth=N` (use `0` for unlimited).
pub const DEFAULT_FLATTEN_DEPTH: usize = 3;

/// Cap on the per-field dedup set for reservoir sampling.
/// Once reached, samples may include duplicates.
const MAX_DEDUP_TRACKING: usize = 1024;

/// HLL error rate (~1.04% standard error, matching the tracking module).
const HLL_ERROR_RATE: f64 = 0.01;

/// Fixed seed for deterministic HLL hashing.
const HLL_SEED: u128 = 0x6669656c645f646973636f76657279; // "field_discovery" in hex

/// Inferred type label for a Dynamic value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Null,
    Array,
    Map,
    Char,
    Other(std::string::String),
}

impl FieldType {
    /// Classify a Rhai Dynamic value.
    pub fn from_dynamic(value: &Dynamic) -> Self {
        if value.is_unit() {
            FieldType::Null
        } else if value.is_string() {
            FieldType::String
        } else if value.is_int() {
            FieldType::Int
        } else if value.is_float() {
            FieldType::Float
        } else if value.is_bool() {
            FieldType::Bool
        } else if value.is_char() {
            FieldType::Char
        } else if value.is_array() {
            FieldType::Array
        } else if value.is_map() {
            FieldType::Map
        } else {
            FieldType::Other(value.type_name().to_string())
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::String => write!(f, "string"),
            FieldType::Int => write!(f, "int"),
            FieldType::Float => write!(f, "float"),
            FieldType::Bool => write!(f, "bool"),
            FieldType::Null => write!(f, "null"),
            FieldType::Array => write!(f, "array"),
            FieldType::Map => write!(f, "map"),
            FieldType::Char => write!(f, "char"),
            FieldType::Other(s) => write!(f, "{}", s),
        }
    }
}

/// Hybrid exact/estimated cardinality tracker.
enum CardinalityTracker {
    /// Exact tracking for small sets.
    Exact(HashSet<u64>),
    /// HyperLogLog estimation for large sets.
    Estimated(HyperLogLog),
}

impl CardinalityTracker {
    fn new() -> Self {
        CardinalityTracker::Exact(HashSet::new())
    }

    /// Insert a value hash. Graduates to HLL when threshold is exceeded.
    fn insert(&mut self, hash: u64) {
        match self {
            CardinalityTracker::Exact(set) => {
                set.insert(hash);
                if set.len() > EXACT_CARDINALITY_THRESHOLD {
                    // Graduate to HLL
                    let mut hll = HyperLogLog::new_deterministic(HLL_ERROR_RATE, HLL_SEED);
                    for &existing in set.iter() {
                        hll.insert(&existing);
                    }
                    *self = CardinalityTracker::Estimated(hll);
                }
            }
            CardinalityTracker::Estimated(hll) => {
                hll.insert(&hash);
            }
        }
    }

    /// Returns `(count, is_exact)`.
    fn cardinality(&self) -> (usize, bool) {
        match self {
            CardinalityTracker::Exact(set) => (set.len(), true),
            CardinalityTracker::Estimated(hll) => (hll.len() as usize, false),
        }
    }
}

/// Per-field profile accumulated across all events.
pub struct FieldProfile {
    /// How many events contained this field.
    pub seen_count: usize,
    /// Type → occurrence count.
    pub type_counts: HashMap<FieldType, usize>,
    /// Cardinality tracker (skipped for map/array types).
    cardinality: CardinalityTracker,
    /// Reservoir sample of distinct values as JSON-compatible scalars.
    pub samples: Vec<serde_json::Value>,
    /// Hashes of values currently present in `samples` plus deduplication history.
    /// Capped at `MAX_DEDUP_TRACKING` to bound memory.
    sample_hashes: HashSet<u64>,
    /// Count of distinct values attempted (for Algorithm R indexing).
    distinct_samples_seen: usize,
    /// Size range for array values: (min_len, max_len).
    pub array_size_range: Option<(usize, usize)>,
    /// Size range for map values: (min_len, max_len).
    pub map_size_range: Option<(usize, usize)>,
}

impl FieldProfile {
    fn new() -> Self {
        Self {
            seen_count: 0,
            type_counts: HashMap::new(),
            cardinality: CardinalityTracker::new(),
            samples: Vec::new(),
            sample_hashes: HashSet::new(),
            distinct_samples_seen: 0,
            array_size_range: None,
            map_size_range: None,
        }
    }

    /// Observe a field value from one event.
    fn observe(&mut self, value: &Dynamic) {
        self.seen_count += 1;

        let ft = FieldType::from_dynamic(value);
        *self.type_counts.entry(ft.clone()).or_insert(0) += 1;

        match ft {
            FieldType::Null => {
                // Don't contribute to cardinality or samples
            }
            FieldType::Array => {
                if let Ok(arr) = value.clone().into_array() {
                    let len = arr.len();
                    self.array_size_range = Some(match self.array_size_range {
                        Some((lo, hi)) => (lo.min(len), hi.max(len)),
                        None => (len, len),
                    });
                }
            }
            FieldType::Map => {
                if let Some(map) = value.clone().try_cast::<rhai::Map>() {
                    let len = map.len();
                    self.map_size_range = Some(match self.map_size_range {
                        Some((lo, hi)) => (lo.min(len), hi.max(len)),
                        None => (len, len),
                    });
                }
            }
            _ => {
                // Scalar: track cardinality and samples
                let display = scalar_display(value);
                let sample_value = scalar_to_json(value);
                let hash = hash_value(&ft, &display);

                self.cardinality.insert(hash);
                self.add_sample(hash, sample_value);
            }
        }
    }

    /// Add a scalar value to the reservoir sample using Algorithm R.
    /// Distinct values are preferred: a bounded hash set deduplicates the
    /// first `MAX_DEDUP_TRACKING` distinct values seen.
    fn add_sample(&mut self, hash: u64, sample: serde_json::Value) {
        if self.sample_hashes.len() < MAX_DEDUP_TRACKING {
            if !self.sample_hashes.insert(hash) {
                return;
            }
        } else if self.sample_hashes.contains(&hash) {
            return;
        }

        self.distinct_samples_seen += 1;

        if self.samples.len() < MAX_SAMPLES {
            self.samples.push(sample);
        } else {
            // Algorithm R: replace random slot with probability K/i.
            let idx = fastrand::usize(0..self.distinct_samples_seen);
            if idx < MAX_SAMPLES {
                self.samples[idx] = sample;
            }
        }
    }

    /// Returns `(count, is_exact)`. For map/array-only fields returns `(0, true)`.
    pub fn cardinality(&self) -> (usize, bool) {
        self.cardinality.cardinality()
    }

    /// Types sorted by frequency (descending).
    pub fn types_by_frequency(&self) -> Vec<(FieldType, usize)> {
        let mut types: Vec<_> = self
            .type_counts
            .iter()
            .map(|(ft, &c)| (ft.clone(), c))
            .collect();
        types.sort_by(|a, b| b.1.cmp(&a.1));
        types
    }
}

/// Accumulator for field discovery across an entire stream.
pub struct FieldDiscovery {
    /// Per-field profiles, insertion-ordered.
    pub fields: IndexMap<std::string::String, FieldProfile>,
    /// Total events observed.
    pub total_events: usize,
    /// Whether we've hit the field cap.
    capped: bool,
    /// Whether nested field flattening stopped at the maximum depth.
    flatten_depth_capped: bool,
    /// Configured depth limit for nested field flattening.
    flatten_depth: usize,
}

impl Default for FieldDiscovery {
    fn default() -> Self {
        Self::with_depth(DEFAULT_FLATTEN_DEPTH)
    }
}

impl FieldDiscovery {
    #[cfg(test)]
    pub fn new() -> Self {
        Self::with_depth(DEFAULT_FLATTEN_DEPTH)
    }

    /// Build a [`FieldDiscovery`] with a specific flatten depth limit.
    ///
    /// A `flatten_depth` of `0` means unlimited: nested maps and arrays will
    /// be flattened all the way down.
    pub fn with_depth(flatten_depth: usize) -> Self {
        Self {
            fields: IndexMap::new(),
            total_events: 0,
            capped: false,
            flatten_depth_capped: false,
            flatten_depth,
        }
    }

    /// Observe all fields from one event.
    ///
    /// Nested maps and arrays are flattened into dotted keys (`user.name`,
    /// `user.roles[]`) up to [`MAX_FLATTEN_DEPTH`] levels deep. The parent
    /// container is also observed so its size range remains visible.
    pub fn observe_event(&mut self, fields: &IndexMap<std::string::String, Dynamic>) {
        self.total_events += 1;

        for (key, value) in fields {
            self.observe_path(key, value, 1);
        }
    }

    /// Observe a value under a given dotted path, then recurse into map/array
    /// children if depth permits.
    ///
    /// A configured `flatten_depth` of `0` means unlimited: recursion only
    /// stops when maps and arrays run out.
    fn observe_path(&mut self, path: &str, value: &Dynamic, depth: usize) {
        self.record(path, value);

        if self.flatten_depth != 0 && depth >= self.flatten_depth {
            if value.is_map() || value.is_array() {
                self.flatten_depth_capped = true;
            }
            return;
        }

        if value.is_map() {
            if let Some(map) = value.clone().try_cast::<rhai::Map>() {
                for (k, v) in map.iter() {
                    let subkey = format!("{path}.{k}");
                    self.observe_path(&subkey, v, depth + 1);
                }
            }
        } else if value.is_array() {
            if let Ok(arr) = value.clone().into_array() {
                let subkey = format!("{path}[]");
                for elem in arr.iter() {
                    self.observe_path(&subkey, elem, depth + 1);
                }
            }
        }
    }

    /// Record a single observation of `value` at the given `path`, honoring
    /// the tracked-fields cap and emitting a one-shot diagnostic on overflow.
    fn record(&mut self, path: &str, value: &Dynamic) {
        if let Some(profile) = self.fields.get_mut(path) {
            profile.observe(value);
        } else {
            if self.fields.len() >= MAX_TRACKED_FIELDS {
                if !self.capped {
                    self.capped = true;
                    eprintln!(
                        "Warning: field discovery truncated at {} unique field names",
                        MAX_TRACKED_FIELDS
                    );
                }
                return;
            }
            let mut profile = FieldProfile::new();
            profile.observe(value);
            self.fields.insert(path.to_string(), profile);
        }
    }

    /// Format the discovery results as a human-readable table.
    pub fn format_table(&self) -> std::string::String {
        let width = if crate::tty::is_stdout_tty() {
            crate::tty::get_terminal_width()
        } else {
            // Honor an explicit COLUMNS override even when piped; otherwise
            // fall back to a generous default so examples have room to expand.
            std::env::var("COLUMNS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|&c| c > 0)
                .unwrap_or(REDIRECTED_TABLE_WIDTH)
        };
        self.format_table_for_width(width)
    }

    fn format_table_for_width(&self, terminal_width: usize) -> std::string::String {
        if self.fields.is_empty() {
            return format!("Scanned {} events: no fields found\n", self.total_events);
        }

        let mut output = std::string::String::new();
        output.push_str(&format!("Scanned {} events\n\n", self.total_events));

        let terminal_width = terminal_width.max(36);

        let mut entries: Vec<_> = self.fields.iter().collect();
        entries.sort_by(|a, b| b.1.seen_count.cmp(&a.1.seen_count));
        let rows: Vec<_> = entries
            .iter()
            .map(|(name, profile)| DiscoveryRow::from_profile(self.total_events, name, profile))
            .collect();

        if let Some(widths) = TableWidths::for_full_table(terminal_width, &rows) {
            output.push_str(&pad_right_display("Field", widths.name));
            output.push_str("  ");
            output.push_str(&pad_right_display("Type", widths.types));
            output.push_str("  ");
            output.push_str(&pad_left_display("Seen", widths.seen));
            output.push_str("  ");
            output.push_str(&pad_left_display("Miss", widths.miss));
            output.push_str("  ");
            output.push_str(&pad_left_display("Uniq", widths.unique));
            output.push_str("  Examples\n");

            for row in &rows {
                output.push_str(&pad_right_display(
                    &truncate_for_display(&row.name, widths.name),
                    widths.name,
                ));
                output.push_str("  ");
                output.push_str(&pad_right_display(
                    &truncate_for_display(&row.types, widths.types),
                    widths.types,
                ));
                output.push_str("  ");
                output.push_str(&pad_left_display(&row.seen_count.to_string(), widths.seen));
                output.push_str("  ");
                output.push_str(&pad_left_display(
                    &format!("{:.0}%", row.miss_pct),
                    widths.miss,
                ));
                output.push_str("  ");
                output.push_str(&pad_left_display(&row.unique, widths.unique));
                output.push_str("  ");
                output.push_str(&truncate_for_display(&row.examples, widths.examples));
                output.push('\n');
            }
        } else if let Some(widths) = TableWidths::for_compact_table(terminal_width, &rows) {
            output.push_str(&pad_right_display("Field", widths.name));
            output.push_str("  ");
            output.push_str(&pad_right_display("Type", widths.types));
            output.push_str("  ");
            output.push_str(&pad_left_display("Seen", widths.seen));
            output.push_str("  ");
            output.push_str(&pad_left_display("Miss", widths.miss));
            output.push_str("  ");
            output.push_str(&pad_left_display("Uniq", widths.unique));
            output.push('\n');

            for row in &rows {
                output.push_str(&pad_right_display(
                    &truncate_for_display(&row.name, widths.name),
                    widths.name,
                ));
                output.push_str("  ");
                output.push_str(&pad_right_display(
                    &truncate_for_display(&row.types, widths.types),
                    widths.types,
                ));
                output.push_str("  ");
                output.push_str(&pad_left_display(&row.seen_count.to_string(), widths.seen));
                output.push_str("  ");
                output.push_str(&pad_left_display(
                    &format!("{:.0}%", row.miss_pct),
                    widths.miss,
                ));
                output.push_str("  ");
                output.push_str(&pad_left_display(&row.unique, widths.unique));
                output.push('\n');
                if !row.examples.is_empty() {
                    output.push_str("  ");
                    output.push_str(&truncate_for_display(
                        &row.examples,
                        terminal_width.saturating_sub(2),
                    ));
                    output.push('\n');
                }
            }
        } else {
            for (idx, row) in rows.iter().enumerate() {
                if idx > 0 {
                    output.push('\n');
                }
                output.push_str(&truncate_for_display(&row.name, terminal_width));
                output.push('\n');
                output.push_str(&format!(
                    "  seen: {}  miss: {:.0}%\n",
                    row.seen_count, row.miss_pct
                ));
                output.push_str("  type: ");
                output.push_str(&truncate_for_display(
                    &row.types,
                    terminal_width.saturating_sub(8),
                ));
                output.push('\n');
                output.push_str("  unique: ");
                output.push_str(&truncate_for_display(
                    &row.unique,
                    terminal_width.saturating_sub(10),
                ));
                output.push('\n');
                if !row.examples.is_empty() {
                    output.push_str("  examples: ");
                    output.push_str(&truncate_for_display(
                        &row.examples,
                        terminal_width.saturating_sub(16),
                    ));
                    output.push('\n');
                }
            }
        }

        if self.capped {
            output.push_str(&format!(
                "\n(Field tracking capped at {} unique field names)\n",
                MAX_TRACKED_FIELDS
            ));
        }
        if self.flatten_depth_capped {
            output.push_str(&format!(
                "\nNote: Nested field flattening stopped at depth {}; deeper children are not shown. Use --discover-depth=N to descend further (0 = unlimited).\n",
                self.flatten_depth
            ));
        }

        output
    }

    /// Format the discovery results as JSON.
    pub fn format_json(&self) -> std::string::String {
        let mut fields_json = Vec::new();

        for (name, profile) in &self.fields {
            let types: Vec<serde_json::Value> = profile
                .types_by_frequency()
                .iter()
                .map(|(ft, count)| {
                    serde_json::json!({
                        "type": ft.to_string(),
                        "count": count,
                    })
                })
                .collect();

            let (card_count, card_exact) = profile.cardinality();

            let mut field_obj = serde_json::json!({
                "name": name,
                "seen": profile.seen_count,
                "missing": self.total_events.saturating_sub(profile.seen_count),
                "types": types,
                "cardinality": {
                    "count": card_count,
                    "exact": card_exact,
                },
                "samples": profile.samples,
            });

            if let Some((lo, hi)) = profile.array_size_range {
                field_obj["array_size"] = serde_json::json!({"min": lo, "max": hi});
            }
            if let Some((lo, hi)) = profile.map_size_range {
                field_obj["map_size"] = serde_json::json!({"min": lo, "max": hi});
            }

            fields_json.push(field_obj);
        }

        let result = serde_json::json!({
            "total_events": self.total_events,
            "fields": fields_json,
            "truncated": self.capped,
            "flatten_depth_limit": self.flatten_depth,
            "flatten_depth_capped": self.flatten_depth_capped,
        });

        serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string())
    }
}

// ── helpers ────────────────────────────────────────────────────────────

/// Format the type column for a field profile.
fn format_types(profile: &FieldProfile) -> std::string::String {
    let types = profile.types_by_frequency();
    if types.is_empty() {
        return "-".to_string();
    }

    let parts: Vec<std::string::String> = types
        .iter()
        .map(|(ft, _count)| match ft {
            FieldType::Array => {
                if let Some((lo, hi)) = profile.array_size_range {
                    if lo == hi {
                        format!("array({})", lo)
                    } else {
                        format!("array({}..{})", lo, hi)
                    }
                } else {
                    "array".to_string()
                }
            }
            FieldType::Map => {
                if let Some((lo, hi)) = profile.map_size_range {
                    if lo == hi {
                        format!("map({})", lo)
                    } else {
                        format!("map({}..{})", lo, hi)
                    }
                } else {
                    "map".to_string()
                }
            }
            _ => ft.to_string(),
        })
        .collect();

    parts.join(", ")
}

/// Format the cardinality column.
fn format_cardinality(profile: &FieldProfile) -> std::string::String {
    let (count, exact) = profile.cardinality();

    // Check if this field is only map/array/null (no scalar cardinality)
    let has_scalar = profile
        .type_counts
        .keys()
        .any(|ft| !matches!(ft, FieldType::Map | FieldType::Array | FieldType::Null));

    if !has_scalar || count == 0 {
        return "\u{2014}".to_string(); // em dash
    }

    if exact {
        format!("{}", count)
    } else {
        format!("~{}", count)
    }
}

/// Format the examples column.
///
/// Produces the full comma-separated list of samples; the caller is
/// responsible for truncating to the available column width at render time.
fn format_examples(profile: &FieldProfile) -> std::string::String {
    if profile.samples.is_empty() {
        return std::string::String::new();
    }

    profile
        .samples
        .iter()
        .map(sample_json_display)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Get a display string for a scalar Dynamic value.
fn scalar_display(value: &Dynamic) -> std::string::String {
    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            return s;
        }
    }
    if value.is_bool() {
        if let Ok(b) = value.as_bool() {
            return b.to_string();
        }
    }
    if value.is_int() {
        if let Ok(i) = value.as_int() {
            return i.to_string();
        }
    }
    if value.is_float() {
        if let Ok(f) = value.as_float() {
            return format!("{f}");
        }
    }
    if value.is_char() {
        if let Ok(c) = value.as_char() {
            return c.to_string();
        }
    }
    value.to_string()
}

/// Convert a scalar Dynamic into a JSON-compatible value for machine-readable
/// discovery output. Non-JSON-native scalar types fall back to strings.
fn scalar_to_json(value: &Dynamic) -> serde_json::Value {
    if value.is_string() {
        value
            .clone()
            .into_string()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null)
    } else if value.is_int() {
        value
            .as_int()
            .map(|i| serde_json::Value::Number(serde_json::Number::from(i)))
            .unwrap_or(serde_json::Value::Null)
    } else if value.is_float() {
        value
            .as_float()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null)
    } else if value.is_bool() {
        value
            .as_bool()
            .map(serde_json::Value::Bool)
            .unwrap_or(serde_json::Value::Null)
    } else if value.is_char() {
        value
            .as_char()
            .map(|c| serde_json::Value::String(c.to_string()))
            .unwrap_or(serde_json::Value::Null)
    } else if value.is_unit() {
        serde_json::Value::Null
    } else {
        serde_json::Value::String(value.to_string())
    }
}

fn sample_json_display(value: &serde_json::Value) -> std::string::String {
    match value {
        // Render strings in inspect-style: escaped and wrapped in double quotes.
        // This makes types unambiguous (so `"42"` is distinguishable from `42`)
        // and naturally surfaces empty strings as `""`.
        //
        // No per-sample length cap is applied here: the layout code truncates
        // the joined examples string to fit the available column width at
        // render time, so long samples can fill a wide terminal in the full
        // table, and the dedicated examples line in the compact table gets to
        // extend all the way to `terminal_width`.
        serde_json::Value::String(s) => {
            format!("\"{}\"", crate::formatters::escape_for_display(s))
        }
        serde_json::Value::Null => "null".to_string(),
        _ => value.to_string(),
    }
}

/// Hash a value with a type prefix to avoid int/string conflation.
fn hash_value(ft: &FieldType, display: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ft.hash(&mut hasher);
    display.hash(&mut hasher);
    hasher.finish()
}

/// Truncate a string to `max_chars` with an ellipsis suffix, preserving valid
/// UTF-8 boundaries.
fn truncate_for_display(s: &str, max_chars: usize) -> std::string::String {
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let char_count = s.chars().count();
    if char_count <= max_chars {
        return s.to_string();
    }

    let keep = max_chars - 3;
    let mut out = s.chars().take(keep).collect::<std::string::String>();
    out.push_str("...");
    out
}

fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

fn pad_right_display(s: &str, width: usize) -> std::string::String {
    let current = display_width(s);
    if current >= width {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(width - current))
}

fn pad_left_display(s: &str, width: usize) -> std::string::String {
    let current = display_width(s);
    if current >= width {
        return s.to_string();
    }
    format!("{}{s}", " ".repeat(width - current))
}

struct TableWidths {
    name: usize,
    seen: usize,
    miss: usize,
    types: usize,
    unique: usize,
    examples: usize,
}

impl TableWidths {
    fn for_full_table(terminal_width: usize, rows: &[DiscoveryRow]) -> Option<Self> {
        let seen = row_width(rows, |row| row.seen_count.to_string().len(), "Seen");
        let miss = 4;
        let unique = row_width(rows, |row| display_width(&row.unique), "Uniq");
        let separators = 10;
        // Layout floors — the minimum width each column would like when there
        // is ample room. They are clamped to the actual content width below so
        // narrow content never inflates the column.
        let layout_min_name = 12;
        let layout_min_types = 6;
        let layout_min_examples = 8;
        let max_name = row_width(rows, |row| display_width(&row.name), "Field").min(40);
        let max_types = row_width(rows, |row| display_width(&row.types), "Type").min(30);
        let max_examples = row_width(rows, |row| display_width(&row.examples), "Examples");
        let available = terminal_width.checked_sub(seen + miss + unique + separators)?;

        // Start each column at its natural floor — the layout minimum capped
        // by the actual content width.
        let floor_name = layout_min_name.min(max_name);
        let floor_types = layout_min_types.min(max_types);
        let floor_examples = layout_min_examples.min(max_examples.max(layout_min_examples));
        if available < floor_name + floor_types + floor_examples {
            return None;
        }

        let mut name = floor_name;
        let mut types = floor_types;
        let mut examples = floor_examples;
        let mut remaining = available.saturating_sub(name + types + examples);

        let name_target = max_name.min(26);
        let type_target = max_types.min(18);

        grow_width(&mut types, type_target, &mut remaining);
        grow_width(&mut name, name_target, &mut remaining);
        grow_width(&mut types, max_types, &mut remaining);
        grow_width(&mut name, max_name, &mut remaining);
        grow_width(&mut examples, max_examples, &mut remaining);

        Some(Self {
            name,
            seen,
            miss,
            types,
            unique,
            examples,
        })
    }

    fn for_compact_table(terminal_width: usize, rows: &[DiscoveryRow]) -> Option<Self> {
        let seen = row_width(rows, |row| row.seen_count.to_string().len(), "Seen");
        let miss = 4;
        let unique = row_width(rows, |row| display_width(&row.unique), "Uniq");
        let separators = 8;
        let layout_min_name = 12;
        let layout_min_types = 6;
        let max_name = row_width(rows, |row| display_width(&row.name), "Field").min(40);
        let max_types = row_width(rows, |row| display_width(&row.types), "Type").min(30);
        let available = terminal_width.checked_sub(seen + miss + unique + separators)?;

        let floor_name = layout_min_name.min(max_name);
        let floor_types = layout_min_types.min(max_types);
        if available < floor_name + floor_types {
            return None;
        }

        // Prefer giving both columns their full content width; if that doesn't
        // fit, shrink the name column (it's the more variable one) while
        // keeping types at content width.
        let (name, types) = if max_name + max_types <= available {
            (max_name, max_types)
        } else {
            let types = max_types.min(available.saturating_sub(floor_name));
            let name = available.saturating_sub(types).min(max_name);
            (name, types)
        };
        if name < floor_name || types < floor_types {
            return None;
        }

        Some(Self {
            name,
            seen,
            miss,
            types,
            unique,
            examples: 0,
        })
    }
}

fn row_width<F>(rows: &[DiscoveryRow], f: F, header: &str) -> usize
where
    F: Fn(&DiscoveryRow) -> usize,
{
    rows.iter()
        .map(f)
        .max()
        .unwrap_or(0)
        .max(display_width(header))
}

fn grow_width(current: &mut usize, target: usize, remaining: &mut usize) {
    if *current >= target || *remaining == 0 {
        return;
    }
    let growth = (target - *current).min(*remaining);
    *current += growth;
    *remaining -= growth;
}

struct DiscoveryRow {
    name: std::string::String,
    seen_count: usize,
    miss_pct: f64,
    types: std::string::String,
    unique: std::string::String,
    examples: std::string::String,
}

impl DiscoveryRow {
    fn from_profile(total_events: usize, name: &str, profile: &FieldProfile) -> Self {
        let missing = total_events.saturating_sub(profile.seen_count);
        let miss_pct = if total_events > 0 {
            (missing as f64 / total_events as f64) * 100.0
        } else {
            0.0
        };

        Self {
            name: name.to_string(),
            seen_count: profile.seen_count,
            miss_pct,
            types: format_types(profile),
            unique: format_cardinality(profile),
            examples: format_examples(profile),
        }
    }
}

// ── thread-local accumulator ──────────────────────────────────────────

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether field discovery is active (set once at startup).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether discovery should observe final emitted fields.
static DISCOVER_FINAL: AtomicBool = AtomicBool::new(false);

/// Configured flatten depth for field discovery (set once at startup).
static FLATTEN_DEPTH: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(DEFAULT_FLATTEN_DEPTH);

thread_local! {
    static THREAD_DISCOVERY: RefCell<FieldDiscovery> = RefCell::new(FieldDiscovery::with_depth(
        FLATTEN_DEPTH.load(Ordering::Relaxed),
    ));
}

/// Enable field discovery (called once at startup).
///
/// A `flatten_depth` of `0` means unlimited: nested maps and arrays will be
/// flattened all the way down.
pub fn enable(discover_final: bool, flatten_depth: usize) {
    FLATTEN_DEPTH.store(flatten_depth, Ordering::Relaxed);
    ENABLED.store(true, Ordering::Relaxed);
    DISCOVER_FINAL.store(discover_final, Ordering::Relaxed);
}

/// Whether field discovery is active.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Whether we should observe final emitted fields.
pub fn is_discover_final() -> bool {
    DISCOVER_FINAL.load(Ordering::Relaxed)
}

/// Observe an event's fields (called from the pipeline).
pub fn observe_event_fields(fields: &IndexMap<String, Dynamic>) {
    if !is_enabled() {
        return;
    }
    THREAD_DISCOVERY.with(|d| d.borrow_mut().observe_event(fields));
}

/// Take the accumulated discovery data from the current thread.
pub fn take_thread_discovery() -> FieldDiscovery {
    let depth = FLATTEN_DEPTH.load(Ordering::Relaxed);
    THREAD_DISCOVERY.with(|d| {
        let mut discovery = d.borrow_mut();
        std::mem::replace(&mut *discovery, FieldDiscovery::with_depth(depth))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_string(s: &str) -> Dynamic {
        Dynamic::from(s.to_string())
    }

    fn make_int(i: i64) -> Dynamic {
        Dynamic::from(i)
    }

    fn make_float(f: f64) -> Dynamic {
        Dynamic::from(f)
    }

    fn make_bool(b: bool) -> Dynamic {
        Dynamic::from(b)
    }

    fn make_null() -> Dynamic {
        Dynamic::UNIT
    }

    fn make_array(items: Vec<Dynamic>) -> Dynamic {
        Dynamic::from(rhai::Array::from(items))
    }

    fn make_map(pairs: Vec<(&str, Dynamic)>) -> Dynamic {
        let mut map = rhai::Map::new();
        for (k, v) in pairs {
            map.insert(k.into(), v);
        }
        Dynamic::from(map)
    }

    #[test]
    fn test_field_type_classification() {
        assert_eq!(
            FieldType::from_dynamic(&make_string("hello")),
            FieldType::String
        );
        assert_eq!(FieldType::from_dynamic(&make_int(42)), FieldType::Int);
        assert_eq!(FieldType::from_dynamic(&make_float(2.5)), FieldType::Float);
        assert_eq!(FieldType::from_dynamic(&make_bool(true)), FieldType::Bool);
        assert_eq!(FieldType::from_dynamic(&make_null()), FieldType::Null);
        assert_eq!(
            FieldType::from_dynamic(&make_array(vec![])),
            FieldType::Array
        );
        assert_eq!(FieldType::from_dynamic(&make_map(vec![])), FieldType::Map);
    }

    #[test]
    fn test_basic_field_profile() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_string("hello"));
        profile.observe(&make_string("world"));
        profile.observe(&make_string("hello")); // duplicate

        assert_eq!(profile.seen_count, 3);
        assert_eq!(profile.type_counts[&FieldType::String], 3);
        let (card, exact) = profile.cardinality();
        assert!(exact);
        assert_eq!(card, 2); // "hello" and "world"
        assert_eq!(profile.samples.len(), 2);
    }

    #[test]
    fn test_mixed_types() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_int(200));
        profile.observe(&make_int(404));
        profile.observe(&make_string("N/A"));

        assert_eq!(profile.seen_count, 3);
        assert_eq!(profile.type_counts[&FieldType::Int], 2);
        assert_eq!(profile.type_counts[&FieldType::String], 1);

        let types = profile.types_by_frequency();
        assert_eq!(types[0].0, FieldType::Int); // most frequent first
    }

    #[test]
    fn test_null_not_counted_in_cardinality() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_null());
        profile.observe(&make_null());
        profile.observe(&make_string("value"));

        assert_eq!(profile.seen_count, 3);
        assert_eq!(profile.type_counts[&FieldType::Null], 2);
        let (card, exact) = profile.cardinality();
        assert!(exact);
        assert_eq!(card, 1); // only "value", nulls not counted
    }

    #[test]
    fn test_int_vs_string_distinct_cardinality() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_int(42));
        profile.observe(&make_string("42"));

        let (card, exact) = profile.cardinality();
        assert!(exact);
        assert_eq!(card, 2); // int:42 != string:"42"
    }

    #[test]
    fn test_array_size_range() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_array(vec![make_int(1), make_int(2)]));
        profile.observe(&make_array(vec![
            make_int(1),
            make_int(2),
            make_int(3),
            make_int(4),
            make_int(5),
        ]));

        assert_eq!(profile.array_size_range, Some((2, 5)));
        let (card, _) = profile.cardinality();
        assert_eq!(card, 0); // arrays don't contribute to cardinality
    }

    #[test]
    fn test_map_size_range() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_map(vec![("a", make_int(1))]));
        profile.observe(&make_map(vec![
            ("a", make_int(1)),
            ("b", make_int(2)),
            ("c", make_int(3)),
        ]));

        assert_eq!(profile.map_size_range, Some((1, 3)));
    }

    #[test]
    fn test_hll_graduation() {
        let mut profile = FieldProfile::new();
        // Insert enough unique values to trigger graduation
        for i in 0..300 {
            profile.observe(&make_int(i));
        }

        let (card, exact) = profile.cardinality();
        assert!(!exact, "Should have graduated to HLL");
        // HLL estimate should be in the ballpark
        assert!(
            (270..=330).contains(&card),
            "HLL estimate {} out of range",
            card
        );
    }

    #[test]
    fn test_field_discovery_basic() {
        let mut discovery = FieldDiscovery::new();

        let mut fields1 = IndexMap::new();
        fields1.insert("level".to_string(), make_string("INFO"));
        fields1.insert("message".to_string(), make_string("hello"));
        fields1.insert("status".to_string(), make_int(200));

        let mut fields2 = IndexMap::new();
        fields2.insert("level".to_string(), make_string("ERROR"));
        fields2.insert("message".to_string(), make_string("fail"));
        // status missing from event 2

        discovery.observe_event(&fields1);
        discovery.observe_event(&fields2);

        assert_eq!(discovery.total_events, 2);
        assert_eq!(discovery.fields.len(), 3);
        assert_eq!(discovery.fields["level"].seen_count, 2);
        assert_eq!(discovery.fields["status"].seen_count, 1);
    }

    #[test]
    fn test_format_table_not_empty() {
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert("level".to_string(), make_string("INFO"));
        fields.insert("msg".to_string(), make_string("test"));
        discovery.observe_event(&fields);

        let table = discovery.format_table();
        assert!(table.contains("Scanned 1 events"));
        assert!(table.contains("level"));
        assert!(table.contains("msg"));
        assert!(table.contains("string"));
    }

    #[test]
    fn test_format_table_compact_layout_on_medium_width() {
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert(
            "very.long.field.name".to_string(),
            make_string("this is a long example value"),
        );
        discovery.observe_event(&fields);

        let table = discovery.format_table_for_width(56);
        assert!(table.contains("Field"));
        assert!(table.contains("Type"));
        assert!(table.contains("Seen"));
        assert!(table.contains("Miss"));
        assert!(table.contains("Uniq"));
        assert!(!table.contains("  examples: "));
        assert!(!table.contains("  seen: "));
    }

    #[test]
    fn test_format_table_narrow_layout_on_small_width() {
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert("request_id".to_string(), make_string("req_001"));
        discovery.observe_event(&fields);

        let table = discovery.format_table_for_width(38);
        assert!(table.contains("request_id"));
        assert!(table.contains("req_001"));
        assert!(table.contains("1"));
        assert!(table.contains("0%"));
        assert!(
            table.contains("  examples: \"req_001\"")
                || table.lines().any(|line| line.starts_with("  \"req_001\"")),
            "{table}"
        );
    }

    #[test]
    fn test_format_json() {
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert("level".to_string(), make_string("INFO"));
        discovery.observe_event(&fields);

        let json = discovery.format_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total_events"], 1);
        assert_eq!(parsed["fields"][0]["name"], "level");
        assert_eq!(parsed["fields"][0]["seen"], 1);
        assert_eq!(parsed["fields"][0]["cardinality"]["exact"], true);
        assert_eq!(parsed["fields"][0]["samples"][0], "INFO");
    }

    #[test]
    fn test_empty_discovery() {
        let discovery = FieldDiscovery::new();
        let table = discovery.format_table();
        assert!(table.contains("Scanned 0 events"));
        assert!(table.contains("no fields found"));
    }

    #[test]
    fn test_sample_limit() {
        let mut profile = FieldProfile::new();
        for i in 0..20 {
            profile.observe(&make_string(&format!("value_{}", i)));
        }
        assert_eq!(profile.samples.len(), MAX_SAMPLES);
    }

    #[test]
    fn test_reservoir_sees_rare_values() {
        // With first-seen, 8 rare values appearing after 1000 common ones would
        // be dropped. Algorithm R should keep some of them in expectation.
        let mut total_rare_in_samples = 0;
        let trials = 40;
        for _ in 0..trials {
            let mut profile = FieldProfile::new();
            for _ in 0..1000 {
                profile.observe(&make_string("common"));
            }
            for i in 0..20 {
                profile.observe(&make_string(&format!("rare_{i}")));
            }
            total_rare_in_samples += profile
                .samples
                .iter()
                .filter(|s| s.as_str().is_some_and(|s| s.starts_with("rare_")))
                .count();
        }
        // Expected ≈ 8 * (20/21) ≈ 7.6 rare samples per trial.
        // With 40 trials we should see rares dominate the reservoir.
        assert!(
            total_rare_in_samples > trials * 4,
            "reservoir should surface rare distinct values; got {total_rare_in_samples} across {trials} trials",
        );
    }

    #[test]
    fn test_scalar_to_json_preserves_scalar_types() {
        assert_eq!(
            scalar_to_json(&make_string("hello")),
            serde_json::json!("hello")
        );
        assert_eq!(scalar_to_json(&make_int(42)), serde_json::json!(42));
        assert_eq!(scalar_to_json(&make_float(2.5)), serde_json::json!(2.5));
        assert_eq!(scalar_to_json(&make_bool(true)), serde_json::json!(true));
        assert_eq!(scalar_to_json(&make_null()), serde_json::Value::Null);
        assert_eq!(scalar_to_json(&Dynamic::from('x')), serde_json::json!("x"));
    }

    #[test]
    fn test_scalar_to_json_preserves_escaped_strings() {
        let value = make_string("line1\nline2\t\"quoted\"\\backslash");
        assert_eq!(
            scalar_to_json(&value),
            serde_json::json!("line1\nline2\t\"quoted\"\\backslash")
        );
    }

    #[test]
    fn test_format_examples_renders_typed_samples_without_mutating_them() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_string("hello"));
        profile.observe(&make_int(42));
        profile.observe(&make_bool(true));

        let before = profile.samples.clone();
        let examples = format_examples(&profile);

        assert!(
            examples.contains("\"hello\""),
            "string sample should render quoted: {examples}"
        );
        assert!(
            examples.contains("42"),
            "int sample should render: {examples}"
        );
        assert!(
            !examples.contains("\"42\""),
            "int sample should not be quoted: {examples}"
        );
        assert!(
            examples.contains("true"),
            "bool sample should render: {examples}"
        );
        assert_eq!(
            profile.samples, before,
            "display formatting must not mutate samples"
        );
    }

    #[test]
    fn test_format_examples_quotes_and_escapes_strings() {
        let mut profile = FieldProfile::new();
        profile.observe(&make_string(""));
        profile.observe(&make_string("a\nb"));
        profile.observe(&make_string("tab\there"));

        let examples = format_examples(&profile);

        assert!(
            examples.contains("\"\""),
            "empty string should render as \"\": {examples}"
        );
        assert!(
            examples.contains("\"a\\nb\""),
            "newlines should be escaped inside quoted strings: {examples}"
        );
        assert!(
            examples.contains("\"tab\\there\""),
            "tabs should be escaped inside quoted strings: {examples}"
        );
    }

    #[test]
    fn test_long_string_samples_preserved_in_format_examples() {
        let long = "x".repeat(200);
        let mut profile = FieldProfile::new();
        profile.observe(&make_string(&long));

        // Profile storage keeps the full length (used for --discover JSON
        // output, which should round-trip the original value).
        assert_eq!(profile.samples.len(), 1);
        assert_eq!(profile.samples[0], serde_json::json!(long));

        // format_examples preserves the full content too; the layout-level
        // truncation in format_table_for_width is what bounds the displayed
        // width, so that compact/full layouts can both extend examples all
        // the way to terminal width on wide terminals.
        let examples = format_examples(&profile);
        assert_eq!(examples, format!("\"{long}\""));
    }

    #[test]
    fn test_compact_examples_line_fills_terminal_width() {
        let long = "y".repeat(300);
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert("sample".to_string(), make_string(&long));
        discovery.observe_event(&fields);

        // Width 40 forces the compact layout (examples on their own line).
        let width = 40;
        let table = discovery.format_table_for_width(width);

        // Find the indented examples line.
        let examples_line = table
            .lines()
            .find(|line| line.starts_with("  \""))
            .unwrap_or_else(|| {
                panic!("expected indented examples line in compact layout:\n{table}")
            });

        // The compact examples line should extend all the way to the terminal
        // width (minus the trailing ellipsis boundary). Before the fix, the
        // per-sample MAX_SAMPLE_LEN=80 cap made this line ~82 chars max — far
        // short of the ~38 chars available after the 2-char indent here; with
        // the cap removed it should fully fill the available width.
        let line_width = display_width(examples_line);
        assert!(
            line_width >= width - 1 && line_width <= width,
            "compact examples line should fill terminal width {width}, got {line_width}: {examples_line}"
        );
    }

    #[test]
    fn test_format_examples_not_capped_at_60_chars() {
        let tags = [
            "alpha_tag",
            "bravo_tag",
            "charlie_tag",
            "delta_tag",
            "echo_tag",
            "foxtrot_tag",
            "golf_tag",
            "hotel_tag",
        ];
        let mut profile = FieldProfile::new();
        for tag in tags {
            profile.observe(&make_string(tag));
        }

        let examples = format_examples(&profile);
        assert!(
            examples.chars().count() > 60,
            "examples should not be capped at 60 chars: {examples}"
        );
        // All samples should be present in full (no mid-sample truncation).
        for tag in tags {
            assert!(
                examples.contains(&format!("\"{tag}\"")),
                "sample {tag:?} missing from examples: {examples}"
            );
        }
    }

    #[test]
    fn test_format_table_uses_wide_terminal_for_examples() {
        let tags = [
            "alpha_tag",
            "bravo_tag",
            "charlie_tag",
            "delta_tag",
            "echo_tag",
            "foxtrot_tag",
            "golf_tag",
            "hotel_tag",
        ];
        let mut discovery = FieldDiscovery::new();
        for tag in tags {
            let mut per_event = IndexMap::new();
            per_event.insert("tag".to_string(), make_string(tag));
            discovery.observe_event(&per_event);
        }

        // At 200 chars wide the full sample list should appear unclipped.
        let table = discovery.format_table_for_width(200);
        for tag in tags {
            assert!(
                table.contains(&format!("\"{tag}\"")),
                "wide table should include full example {tag:?}: {table}"
            );
        }

        // At 60 chars wide the list should be truncated (at least one sample
        // missing or cut off with an ellipsis).
        let table = discovery.format_table_for_width(60);
        assert!(
            table.contains("..."),
            "narrow table should truncate examples: {table}"
        );
    }

    #[test]
    fn test_nested_flattening() {
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert(
            "user".to_string(),
            make_map(vec![("name", make_string("alice")), ("age", make_int(30))]),
        );
        fields.insert("level".to_string(), make_string("INFO"));
        discovery.observe_event(&fields);

        // Parent container retained
        assert!(discovery.fields.contains_key("user"));
        // Children flattened with dotted paths
        assert!(discovery.fields.contains_key("user.name"));
        assert!(discovery.fields.contains_key("user.age"));
        assert_eq!(discovery.fields["user.name"].seen_count, 1);
        assert_eq!(discovery.fields["user.age"].type_counts[&FieldType::Int], 1);
    }

    #[test]
    fn test_array_element_flattening() {
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert(
            "roles".to_string(),
            make_array(vec![make_string("admin"), make_string("dev")]),
        );
        discovery.observe_event(&fields);

        assert!(discovery.fields.contains_key("roles"));
        assert!(discovery.fields.contains_key("roles[]"));
        // Two elements → two observations
        assert_eq!(discovery.fields["roles[]"].seen_count, 2);
        let (card, _) = discovery.fields["roles[]"].cardinality();
        assert_eq!(card, 2);
    }

    #[test]
    fn test_depth_limit() {
        // Build a 5-deep nested map: a.b.c.d.e
        let deep = make_map(vec![(
            "b",
            make_map(vec![(
                "c",
                make_map(vec![("d", make_map(vec![("e", make_string("bottom"))]))]),
            )]),
        )]);
        let mut fields = IndexMap::new();
        fields.insert("a".to_string(), deep);

        let mut discovery = FieldDiscovery::new();
        discovery.observe_event(&fields);

        // Depth 3 means we record a (1), a.b (2), a.b.c (3) and stop there.
        assert!(discovery.fields.contains_key("a"));
        assert!(discovery.fields.contains_key("a.b"));
        assert!(discovery.fields.contains_key("a.b.c"));
        assert!(!discovery.fields.contains_key("a.b.c.d"));
        assert!(!discovery.fields.contains_key("a.b.c.d.e"));

        let table = discovery.format_table();
        assert!(
            table.contains("Nested field flattening stopped at depth 3"),
            "table should make depth cap explicit: {table}"
        );

        let json = discovery.format_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["flatten_depth_limit"], 3);
        assert_eq!(parsed["flatten_depth_capped"], true);
    }

    #[test]
    fn test_depth_limit_unlimited() {
        // Build a 5-deep nested map: a.b.c.d.e
        let deep = make_map(vec![(
            "b",
            make_map(vec![(
                "c",
                make_map(vec![("d", make_map(vec![("e", make_string("bottom"))]))]),
            )]),
        )]);
        let mut fields = IndexMap::new();
        fields.insert("a".to_string(), deep);

        // Depth 0 means unlimited: every level should be recorded.
        let mut discovery = FieldDiscovery::with_depth(0);
        discovery.observe_event(&fields);

        assert!(discovery.fields.contains_key("a"));
        assert!(discovery.fields.contains_key("a.b"));
        assert!(discovery.fields.contains_key("a.b.c"));
        assert!(discovery.fields.contains_key("a.b.c.d"));
        assert!(discovery.fields.contains_key("a.b.c.d.e"));

        let table = discovery.format_table();
        assert!(
            !table.contains("Nested field flattening stopped"),
            "unlimited depth should not emit a depth-cap note: {table}"
        );

        let json = discovery.format_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["flatten_depth_limit"], 0);
        assert_eq!(parsed["flatten_depth_capped"], false);
    }

    #[test]
    fn test_array_seen_exceeds_events_does_not_panic() {
        // Array-element fields can have seen_count > total_events.
        let mut discovery = FieldDiscovery::new();
        let mut fields = IndexMap::new();
        fields.insert(
            "tags".to_string(),
            make_array(vec![make_string("a"), make_string("b"), make_string("c")]),
        );
        discovery.observe_event(&fields);

        // seen_count=3 > total_events=1 for tags[]
        assert_eq!(discovery.fields["tags[]"].seen_count, 3);
        assert_eq!(discovery.total_events, 1);

        // Formatting must not panic
        let table = discovery.format_table();
        assert!(table.contains("tags[]"));
        let json = discovery.format_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["truncated"], false);
        assert_eq!(parsed["flatten_depth_limit"], 3);
        assert_eq!(parsed["flatten_depth_capped"], false);
    }
}
