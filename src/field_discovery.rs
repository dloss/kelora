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

/// Threshold at which we graduate from exact tracking to HLL estimation.
const EXACT_CARDINALITY_THRESHOLD: usize = 256;

/// Maximum number of sample values to keep per field.
const MAX_SAMPLES: usize = 8;

/// Maximum character length for stored sample values.
const MAX_SAMPLE_LEN: usize = 80;

/// Maximum number of distinct fields to track (memory safety).
const MAX_TRACKED_FIELDS: usize = 1_000;

/// Maximum depth for flattening nested maps and arrays into dotted keys.
/// Depth counts descents from the event root: `a.b.c` is depth 3.
const MAX_FLATTEN_DEPTH: usize = 3;

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
    /// Reservoir sample of distinct values (display strings).
    pub samples: Vec<std::string::String>,
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
                let hash = hash_value(&ft, &display);

                self.cardinality.insert(hash);
                self.add_sample(hash, &display);
            }
        }
    }

    /// Add a scalar value to the reservoir sample using Algorithm R.
    /// Distinct values are preferred: a bounded hash set deduplicates the
    /// first `MAX_DEDUP_TRACKING` distinct values seen.
    fn add_sample(&mut self, hash: u64, display: &str) {
        if self.sample_hashes.len() < MAX_DEDUP_TRACKING {
            if !self.sample_hashes.insert(hash) {
                return;
            }
        } else if self.sample_hashes.contains(&hash) {
            return;
        }

        self.distinct_samples_seen += 1;

        if self.samples.len() < MAX_SAMPLES {
            self.samples.push(truncate_sample(display));
        } else {
            // Algorithm R: replace random slot with probability K/i.
            let idx = fastrand::usize(0..self.distinct_samples_seen);
            if idx < MAX_SAMPLES {
                self.samples[idx] = truncate_sample(display);
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
}

impl FieldDiscovery {
    pub fn new() -> Self {
        Self {
            fields: IndexMap::new(),
            total_events: 0,
            capped: false,
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
    fn observe_path(&mut self, path: &str, value: &Dynamic, depth: usize) {
        self.record(path, value);

        if depth >= MAX_FLATTEN_DEPTH {
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
                        "⚠️ field discovery truncated at {} unique field names",
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
        if self.fields.is_empty() {
            return format!(
                "Field Discovery ({} events scanned): no fields found\n",
                self.total_events
            );
        }

        let mut output = std::string::String::new();
        output.push_str(&format!(
            "Field Discovery ({} events scanned):\n\n",
            self.total_events
        ));

        // Compute column widths
        let name_width = self
            .fields
            .keys()
            .map(|k| k.len())
            .max()
            .unwrap_or(5)
            .clamp(5, 40);
        let seen_width = 6; // "Seen" column
        let miss_width = 5; // "Miss%" column
        let types_width = self
            .fields
            .values()
            .map(|p| format_types(p).len())
            .max()
            .unwrap_or(5)
            .clamp(5, 30);
        let unique_width = 8; // "Unique" column

        // Header
        output.push_str(&format!(
            "{:<name_w$}  {:>seen_w$}  {:>miss_w$}  {:<types_w$}  {:>unique_w$}  Examples\n",
            "Field",
            "Seen",
            "Miss%",
            "Types",
            "Unique",
            name_w = name_width,
            seen_w = seen_width,
            miss_w = miss_width,
            types_w = types_width,
            unique_w = unique_width,
        ));

        // Separator
        let total_width =
            name_width + seen_width + miss_width + types_width + unique_width + 12 + 20;
        output.push_str(&"\u{2500}".repeat(total_width.min(120)));
        output.push('\n');

        // Rows — sort by seen_count descending for relevance
        let mut entries: Vec<_> = self.fields.iter().collect();
        entries.sort_by(|a, b| b.1.seen_count.cmp(&a.1.seen_count));

        for (name, profile) in &entries {
            // Flattened array-element fields (`key[]`) can have seen_count > total_events
            // because each element counts as an observation; saturating_sub avoids underflow.
            let missing = self.total_events.saturating_sub(profile.seen_count);
            let miss_pct = if self.total_events > 0 {
                (missing as f64 / self.total_events as f64) * 100.0
            } else {
                0.0
            };

            let types_str = format_types(profile);
            let unique_str = format_cardinality(profile);
            let examples_str = format_examples(profile);

            // Truncate long field names
            let display_name = if name.len() > name_width {
                format!("{}...", &name[..name_width - 3])
            } else {
                name.to_string()
            };

            output.push_str(&format!(
                "{:<name_w$}  {:>seen_w$}  {:>miss_w$.0}%  {:<types_w$}  {:>unique_w$}  {}\n",
                display_name,
                profile.seen_count,
                miss_pct,
                types_str,
                unique_str,
                examples_str,
                name_w = name_width,
                seen_w = seen_width,
                miss_w = miss_width,
                types_w = types_width,
                unique_w = unique_width,
            ));
        }

        if self.capped {
            output.push_str(&format!(
                "\n(Field tracking capped at {} unique field names)\n",
                MAX_TRACKED_FIELDS
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
fn format_examples(profile: &FieldProfile) -> std::string::String {
    if profile.samples.is_empty() {
        return std::string::String::new();
    }

    let joined = profile.samples.join(", ");
    if joined.len() > 60 {
        format!("{}...", &joined[..57])
    } else {
        joined
    }
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

/// Hash a value with a type prefix to avoid int/string conflation.
fn hash_value(ft: &FieldType, display: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ft.hash(&mut hasher);
    display.hash(&mut hasher);
    hasher.finish()
}

/// Truncate a sample value for display.
fn truncate_sample(s: &str) -> std::string::String {
    if s.len() <= MAX_SAMPLE_LEN {
        s.to_string()
    } else {
        format!("{}...", &s[..MAX_SAMPLE_LEN - 3])
    }
}

// ── thread-local accumulator ──────────────────────────────────────────

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether field discovery is active (set once at startup).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Which observation point to use.
static SCOPE_OUTPUT: AtomicBool = AtomicBool::new(false);

thread_local! {
    static THREAD_DISCOVERY: RefCell<FieldDiscovery> = RefCell::new(FieldDiscovery::new());
}

/// Enable field discovery (called once at startup).
pub fn enable(output_scope: bool) {
    ENABLED.store(true, Ordering::Relaxed);
    SCOPE_OUTPUT.store(output_scope, Ordering::Relaxed);
}

/// Whether field discovery is active.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Whether we should observe at the output (post-filter) point.
pub fn is_output_scope() -> bool {
    SCOPE_OUTPUT.load(Ordering::Relaxed)
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
    THREAD_DISCOVERY.with(|d| {
        let mut discovery = d.borrow_mut();
        std::mem::replace(&mut *discovery, FieldDiscovery::new())
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
        assert!(table.contains("Field Discovery"));
        assert!(table.contains("1 events scanned"));
        assert!(table.contains("level"));
        assert!(table.contains("msg"));
        assert!(table.contains("string"));
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
    }

    #[test]
    fn test_empty_discovery() {
        let discovery = FieldDiscovery::new();
        let table = discovery.format_table();
        assert!(table.contains("0 events scanned"));
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
                .filter(|s| s.starts_with("rare_"))
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
    }
}
