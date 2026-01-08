//! Thread-safe state tracking for parallel processing
//!
//! Contains GlobalTracker for merging worker states and statistics,
//! along with helper functions for TDigest serialization.

use anyhow::Result;
use hyperloglog::HyperLogLog;
use rhai::Dynamic;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tdigests::TDigest;

use crate::rhai_functions::tracking::TrackingSnapshot;
use crate::stats::ProcessingStats;

/// Helper function to serialize a TDigest to bytes for storage in Dynamic
pub(crate) fn serialize_tdigest(digest: &TDigest) -> Vec<u8> {
    let centroids = digest.centroids();
    let mut bytes = Vec::new();

    // Store number of centroids (8 bytes)
    let count = centroids.len();
    bytes.extend_from_slice(&count.to_le_bytes());

    // Store each centroid (mean: f64, weight: f64 = 16 bytes each)
    for centroid in centroids {
        bytes.extend_from_slice(&centroid.mean.to_le_bytes());
        bytes.extend_from_slice(&centroid.weight.to_le_bytes());
    }

    bytes
}

/// Helper function to deserialize a TDigest from bytes stored in Dynamic
pub(crate) fn deserialize_tdigest(bytes: &[u8]) -> Option<TDigest> {
    if bytes.len() < 8 {
        return None;
    }

    // Read number of centroids
    let count = usize::from_le_bytes(bytes[0..8].try_into().ok()?);

    if bytes.len() < 8 + count * 16 {
        return None;
    }

    // Reconstruct centroids
    let mut centroids = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 8 + i * 16;
        let mean = f64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        let weight = f64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().ok()?);
        centroids.push(tdigests::Centroid::new(mean, weight));
    }

    // Reconstruct t-digest from centroids
    Some(TDigest::from_centroids(centroids))
}

/// Magic bytes to identify HLL blobs (must match tracking.rs)
const HLL_MAGIC: &[u8; 4] = b"HLL\x01";

/// Helper function to serialize a HyperLogLog to bytes for storage in Dynamic
pub(crate) fn serialize_hll(hll: &HyperLogLog) -> Vec<u8> {
    let mut bytes = Vec::new();

    // Magic bytes to identify this as HLL (4 bytes)
    bytes.extend_from_slice(HLL_MAGIC);

    // Serialize HLL using serde_json (must match tracking.rs)
    if let Ok(json) = serde_json::to_vec(hll) {
        bytes.extend_from_slice(&json);
    }

    bytes
}

/// Helper function to deserialize a HyperLogLog from bytes stored in Dynamic
pub(crate) fn deserialize_hll(bytes: &[u8]) -> Option<HyperLogLog> {
    // Check magic bytes
    if bytes.len() < 4 || &bytes[0..4] != HLL_MAGIC {
        return None;
    }

    // Deserialize HLL from JSON
    serde_json::from_slice(&bytes[4..]).ok()
}

/// Thread-safe statistics tracker for merging worker states
#[derive(Debug, Default, Clone)]
pub struct GlobalTracker {
    pub(crate) user_tracked: Arc<Mutex<HashMap<String, Dynamic>>>,
    pub(crate) internal_tracked: Arc<Mutex<HashMap<String, Dynamic>>>,
    pub(crate) processing_stats: Arc<Mutex<ProcessingStats>>,
    pub(crate) start_time: Option<Instant>,
}

impl GlobalTracker {
    pub fn new() -> Self {
        Self {
            user_tracked: Arc::new(Mutex::new(HashMap::new())),
            internal_tracked: Arc::new(Mutex::new(HashMap::new())),
            processing_stats: Arc::new(Mutex::new(ProcessingStats::new())),
            start_time: Some(Instant::now()),
        }
    }

    /// Lock processing stats with poison recovery
    fn lock_stats(&self) -> std::sync::MutexGuard<'_, ProcessingStats> {
        match self.processing_stats.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("⚠️  Worker thread panicked, recovering processing stats");
                poisoned.into_inner()
            }
        }
    }

    /// Lock user tracked state with poison recovery
    fn lock_user_tracked(&self) -> std::sync::MutexGuard<'_, HashMap<String, Dynamic>> {
        match self.user_tracked.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("⚠️  Worker thread panicked, recovering user tracked state");
                poisoned.into_inner()
            }
        }
    }

    /// Lock internal tracked state with poison recovery
    pub(crate) fn lock_internal_tracked(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, Dynamic>> {
        match self.internal_tracked.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                eprintln!("⚠️  Worker thread panicked, recovering internal tracked state");
                poisoned.into_inner()
            }
        }
    }

    pub fn merge_worker_stats(&self, worker_stats: &ProcessingStats) -> Result<()> {
        let mut global_stats = self.lock_stats();
        // Don't merge lines_read - that's handled by reader thread
        // Merge error counts (needed for --stats display and termination case)
        global_stats.lines_errors += worker_stats.lines_errors;
        global_stats.errors += worker_stats.errors;
        // Merge other worker stats
        global_stats.files_processed += worker_stats.files_processed;
        global_stats.script_executions += worker_stats.script_executions;
        global_stats.timestamp_detected_events += worker_stats.timestamp_detected_events;
        global_stats.timestamp_parsed_events += worker_stats.timestamp_parsed_events;
        global_stats.timestamp_absent_events += worker_stats.timestamp_absent_events;
        global_stats.yearless_timestamps += worker_stats.yearless_timestamps;
        global_stats.timestamp_override_failed |= worker_stats.timestamp_override_failed;
        if global_stats.timestamp_override_field.is_none() {
            if let Some(field) = &worker_stats.timestamp_override_field {
                global_stats.timestamp_override_field = Some(field.clone());
            }
        }
        if global_stats.timestamp_override_format.is_none() {
            if let Some(format) = &worker_stats.timestamp_override_format {
                global_stats.timestamp_override_format = Some(format.clone());
            }
        }
        if global_stats.timestamp_override_warning.is_none() {
            if let Some(message) = &worker_stats.timestamp_override_warning {
                global_stats.timestamp_override_warning = Some(message.clone());
            }
        }

        for (field, worker_field_stats) in &worker_stats.timestamp_fields {
            let entry = global_stats
                .timestamp_fields
                .entry(field.clone())
                .or_default();
            entry.detected += worker_field_stats.detected;
            entry.parsed += worker_field_stats.parsed;
        }
        // Calculate total processing time from global start time
        if let Some(start_time) = self.start_time {
            global_stats.processing_time = start_time.elapsed();
        }
        Ok(())
    }

    pub fn extract_final_stats_from_tracking(
        &self,
        metrics: &HashMap<String, Dynamic>,
    ) -> Result<()> {
        let mut stats = self.lock_stats();

        let output = metrics
            .get("__kelora_stats_output")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0) as usize;
        // Note: Line-level filtering is not used - all filtering is done at event level
        let lines_errors = metrics
            .get("__kelora_stats_lines_errors")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0) as usize;
        let events_created = metrics
            .get("__kelora_stats_events_created")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0) as usize;
        let events_output = metrics
            .get("__kelora_stats_events_output")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0) as usize;
        let events_filtered = metrics
            .get("__kelora_stats_events_filtered")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0) as usize;

        stats.lines_output = output;
        stats.lines_errors = lines_errors;
        stats.errors = lines_errors; // Keep errors field for backward compatibility
        stats.events_created = events_created;
        stats.events_output = events_output;
        stats.events_filtered = events_filtered;

        // Extract discovered levels from tracking data
        if let Some(levels_dynamic) = metrics.get("__kelora_stats_discovered_levels") {
            if let Ok(levels_array) = levels_dynamic.clone().into_array() {
                for level in levels_array {
                    if let Ok(level_str) = level.into_string() {
                        stats.discovered_levels.insert(level_str);
                    }
                }
            }
        }

        // Extract discovered keys from tracking data
        if let Some(keys_dynamic) = metrics.get("__kelora_stats_discovered_keys") {
            if let Ok(keys_array) = keys_dynamic.clone().into_array() {
                for key in keys_array {
                    if let Ok(key_str) = key.into_string() {
                        stats.discovered_keys.insert(key_str);
                    }
                }
            }
        }

        // Extract discovered levels output from tracking data
        if let Some(levels_dynamic) = metrics.get("__kelora_stats_discovered_levels_output") {
            if let Ok(levels_array) = levels_dynamic.clone().into_array() {
                for level in levels_array {
                    if let Ok(level_str) = level.into_string() {
                        stats.discovered_levels_output.insert(level_str);
                    }
                }
            }
        }

        // Extract discovered keys output from tracking data
        if let Some(keys_dynamic) = metrics.get("__kelora_stats_discovered_keys_output") {
            if let Ok(keys_array) = keys_dynamic.clone().into_array() {
                for key in keys_array {
                    if let Ok(key_str) = key.into_string() {
                        stats.discovered_keys_output.insert(key_str);
                    }
                }
            }
        }

        Ok(())
    }

    pub fn get_final_stats(&self) -> ProcessingStats {
        let mut stats = self.lock_stats().clone();
        // Ensure we have the latest processing time
        if let Some(start_time) = self.start_time {
            stats.processing_time = start_time.elapsed();
        }
        stats
    }

    pub fn set_total_lines_read(&self, total_lines: usize) -> Result<()> {
        let mut global_stats = self.lock_stats();
        global_stats.lines_read = total_lines;
        Ok(())
    }

    pub fn add_lines_filtered(&self, count: usize) -> Result<()> {
        let mut global_stats = self.lock_stats();
        global_stats.lines_filtered += count;
        Ok(())
    }

    pub fn merge_worker_state(
        &self,
        user_state: HashMap<String, Dynamic>,
        internal_state: HashMap<String, Dynamic>,
    ) -> Result<()> {
        {
            let mut global_user = self.lock_user_tracked();
            Self::merge_state_with_lookup(
                &mut global_user,
                &user_state,
                |op_key| user_state.get(op_key).cloned(),
                false,
            );
        }

        {
            let mut global_internal = self.lock_internal_tracked();
            Self::merge_state_with_lookup(
                &mut global_internal,
                &internal_state,
                |op_key| internal_state.get(op_key).cloned(),
                true,
            );
        }

        Ok(())
    }

    /// Merge top or bottom tracking arrays from parallel workers.
    /// Handles both count mode (sum counts) and weighted mode (max/min values).
    ///
    /// # Arguments
    /// * `existing_arr` - Array from first worker
    /// * `new_arr` - Array from second worker
    /// * `is_top` - true for top-N (descending, max), false for bottom-N (ascending, min)
    fn merge_top_bottom_arrays(
        existing_arr: rhai::Array,
        new_arr: rhai::Array,
        is_top: bool,
    ) -> rhai::Array {
        // Capture N from original array sizes before consuming arrays
        let n = existing_arr.len().max(new_arr.len());

        // Merge arrays from both workers
        let mut merged_map: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();

        // Determine if this is count mode or weighted mode
        let field_name = if !existing_arr.is_empty() {
            if let Some(first_map) = existing_arr[0].clone().try_cast::<rhai::Map>() {
                if first_map.contains_key("count") {
                    "count"
                } else {
                    "value"
                }
            } else {
                "count"
            }
        } else {
            "count"
        };

        // Merge existing array
        for item in existing_arr {
            if let Some(map) = item.try_cast::<rhai::Map>() {
                if let (Some(k), Some(v)) = (map.get("key"), map.get(field_name)) {
                    if let Ok(key_str) = k.clone().into_string() {
                        let val = if field_name == "count" {
                            v.as_int().unwrap_or(0) as f64
                        } else {
                            v.as_float().unwrap_or(0.0)
                        };
                        merged_map.insert(key_str, val);
                    }
                }
            }
        }

        // Merge new array (for count: add counts, for value: take max/min based on is_top)
        for item in new_arr {
            if let Some(map) = item.try_cast::<rhai::Map>() {
                if let (Some(k), Some(v)) = (map.get("key"), map.get(field_name)) {
                    if let Ok(key_str) = k.clone().into_string() {
                        let val = if field_name == "count" {
                            v.as_int().unwrap_or(0) as f64
                        } else {
                            v.as_float().unwrap_or(0.0)
                        };

                        if field_name == "count" {
                            // Count mode: sum counts
                            *merged_map.entry(key_str).or_insert(0.0) += val;
                        } else {
                            // Weighted mode: take max (top) or min (bottom)
                            if is_top {
                                merged_map
                                    .entry(key_str)
                                    .and_modify(|e| *e = e.max(val))
                                    .or_insert(val);
                            } else {
                                merged_map
                                    .entry(key_str)
                                    .and_modify(|e| *e = e.min(val))
                                    .or_insert(val);
                            }
                        }
                    }
                }
            }
        }

        // Convert to vec and sort
        let mut items: Vec<(String, f64)> = merged_map.into_iter().collect();
        if is_top {
            // Top: descending by value, ascending by key
            items.sort_by(|a, b| {
                match b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal) {
                    std::cmp::Ordering::Equal => a.0.cmp(&b.0),
                    other => other,
                }
            });
        } else {
            // Bottom: ascending by value, ascending by key
            items.sort_by(|a, b| {
                match a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal) {
                    std::cmp::Ordering::Equal => a.0.cmp(&b.0),
                    other => other,
                }
            });
        }

        // Trim to top/bottom N
        if items.len() > n {
            items.truncate(n);
        }

        // Convert back to rhai array of maps
        items
            .into_iter()
            .map(|(k, v)| {
                let mut map = rhai::Map::new();
                map.insert("key".into(), Dynamic::from(k));
                if field_name == "count" {
                    map.insert("count".into(), Dynamic::from(v as i64));
                } else {
                    map.insert("value".into(), Dynamic::from(v));
                }
                Dynamic::from(map)
            })
            .collect()
    }

    /// Merge numeric values (int or float) using addition
    fn merge_numeric_add(existing: &Dynamic, value: &Dynamic) -> Dynamic {
        if existing.is_float() || value.is_float() {
            let a = if existing.is_float() {
                existing.as_float().unwrap_or(0.0)
            } else {
                existing.as_int().unwrap_or(0) as f64
            };
            let b = if value.is_float() {
                value.as_float().unwrap_or(0.0)
            } else {
                value.as_int().unwrap_or(0) as f64
            };
            Dynamic::from(a + b)
        } else {
            let a = existing.as_int().unwrap_or(0);
            let b = value.as_int().unwrap_or(0);
            Dynamic::from(a + b)
        }
    }

    /// Merge average tracking by combining sums and counts
    fn merge_avg(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        let existing_map = existing.clone().try_cast::<rhai::Map>()?;
        let new_map = value.clone().try_cast::<rhai::Map>()?;

        let existing_sum = existing_map
            .get("sum")
            .and_then(|v| {
                if v.is_float() {
                    v.as_float().ok()
                } else if v.is_int() {
                    v.as_int().ok().map(|i| i as f64)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let existing_count = existing_map
            .get("count")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0);

        let new_sum = new_map
            .get("sum")
            .and_then(|v| {
                if v.is_float() {
                    v.as_float().ok()
                } else if v.is_int() {
                    v.as_int().ok().map(|i| i as f64)
                } else {
                    None
                }
            })
            .unwrap_or(0.0);
        let new_count = new_map
            .get("count")
            .and_then(|v| v.as_int().ok())
            .unwrap_or(0);

        let mut merged = rhai::Map::new();
        merged.insert("sum".into(), Dynamic::from(existing_sum + new_sum));
        merged.insert("count".into(), Dynamic::from(existing_count + new_count));
        Some(Dynamic::from(merged))
    }

    /// Merge min values (returns smallest)
    fn merge_min(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        if let (Ok(a), Ok(b)) = (existing.as_int(), value.as_int()) {
            Some(Dynamic::from(a.min(b)))
        } else {
            None
        }
    }

    /// Merge max values (returns largest)
    fn merge_max(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        if let (Ok(a), Ok(b)) = (existing.as_int(), value.as_int()) {
            Some(Dynamic::from(a.max(b)))
        } else {
            None
        }
    }

    /// Merge unique arrays (no duplicates)
    fn merge_unique(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        let existing_arr = existing.clone().into_array().ok()?;
        let new_arr = value.clone().into_array().ok()?;

        let mut merged = existing_arr;
        for item in new_arr {
            if !merged.iter().any(|v| v.to_string() == item.to_string()) {
                merged.push(item);
            }
        }
        Some(Dynamic::from(merged))
    }

    /// Merge bucket maps (sum counts per bucket)
    fn merge_bucket(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        let existing_map = existing.clone().try_cast::<rhai::Map>()?;
        let new_map = value.clone().try_cast::<rhai::Map>()?;

        let mut merged = existing_map;
        for (bucket_key, bucket_value) in new_map {
            if let Ok(bucket_count) = bucket_value.as_int() {
                let existing_count = merged
                    .get(&bucket_key)
                    .and_then(|v| v.as_int().ok())
                    .unwrap_or(0);
                merged.insert(bucket_key, Dynamic::from(existing_count + bucket_count));
            }
        }
        Some(Dynamic::from(merged))
    }

    /// Merge error_examples arrays (limit to 3 unique items)
    fn merge_error_examples(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        let existing_arr = existing.clone().into_array().ok()?;
        let new_arr = value.clone().into_array().ok()?;

        let mut merged = existing_arr;
        for item in new_arr {
            if merged.len() < 3 && !merged.iter().any(|v| v.to_string() == item.to_string()) {
                merged.push(item);
            }
        }
        Some(Dynamic::from(merged))
    }

    /// Merge percentiles (t-digest sketches)
    fn merge_percentiles(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        let existing_blob = existing.clone().into_blob().ok()?;
        let new_blob = value.clone().into_blob().ok()?;

        // Deserialize both t-digests
        let existing_digest = deserialize_tdigest(&existing_blob)?;
        let new_digest = deserialize_tdigest(&new_blob)?;

        // Merge the digests using the merge method
        let merged_digest = existing_digest.merge(&new_digest);

        // Serialize and store
        let bytes = serialize_tdigest(&merged_digest);
        Some(Dynamic::from_blob(bytes))
    }

    fn merge_cardinality(existing: &Dynamic, value: &Dynamic) -> Option<Dynamic> {
        let existing_blob = existing.clone().into_blob().ok()?;
        let new_blob = value.clone().into_blob().ok()?;

        // Deserialize both HyperLogLogs
        let existing_hll = deserialize_hll(&existing_blob)?;
        let new_hll = deserialize_hll(&new_blob)?;

        // Merge the HLLs
        let mut merged_hll = existing_hll;
        merged_hll.merge(&new_hll);

        // Serialize and store
        let bytes = serialize_hll(&merged_hll);
        Some(Dynamic::from_blob(bytes))
    }

    fn merge_state_with_lookup<F>(
        target: &mut HashMap<String, Dynamic>,
        worker_state: &HashMap<String, Dynamic>,
        mut op_lookup: F,
        copy_metadata: bool,
    ) where
        F: FnMut(&str) -> Option<Dynamic>,
    {
        for (key, value) in worker_state {
            if key.starts_with("__op_") {
                if copy_metadata {
                    target.insert(key.clone(), value.clone());
                }
                continue;
            }

            if let Some(existing) = target.get(key) {
                let op_key = format!("__op_{}", key);
                let operation = op_lookup(&op_key)
                    .and_then(|v| v.into_string().ok())
                    .unwrap_or_else(|| "replace".to_string());

                match operation.as_str() {
                    "count" | "sum" => {
                        let merged = Self::merge_numeric_add(existing, value);
                        target.insert(key.clone(), merged);
                        continue;
                    }
                    "avg" => {
                        if let Some(merged) = Self::merge_avg(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "min" => {
                        if let Some(merged) = Self::merge_min(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "max" => {
                        if let Some(merged) = Self::merge_max(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "unique" => {
                        if let Some(merged) = Self::merge_unique(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "bucket" => {
                        if let Some(merged) = Self::merge_bucket(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "top" => {
                        if let (Ok(existing_arr), Ok(new_arr)) =
                            (existing.clone().into_array(), value.clone().into_array())
                        {
                            let result_arr =
                                Self::merge_top_bottom_arrays(existing_arr, new_arr, true);
                            target.insert(key.clone(), Dynamic::from(result_arr));
                            continue;
                        }
                    }
                    "bottom" => {
                        if let (Ok(existing_arr), Ok(new_arr)) =
                            (existing.clone().into_array(), value.clone().into_array())
                        {
                            let result_arr =
                                Self::merge_top_bottom_arrays(existing_arr, new_arr, false);
                            target.insert(key.clone(), Dynamic::from(result_arr));
                            continue;
                        }
                    }
                    "error_examples" => {
                        if let Some(merged) = Self::merge_error_examples(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "percentiles" => {
                        if let Some(merged) = Self::merge_percentiles(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    "cardinality" => {
                        if let Some(merged) = Self::merge_cardinality(existing, value) {
                            target.insert(key.clone(), merged);
                            continue;
                        }
                    }
                    _ => {}
                }
                target.insert(key.clone(), value.clone());
            } else {
                target.insert(key.clone(), value.clone());
            }
        }
    }

    pub fn get_final_snapshot(&self) -> TrackingSnapshot {
        let user = self.lock_user_tracked().clone();
        let internal = self.lock_internal_tracked().clone();
        TrackingSnapshot::from_parts(user, internal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{Dynamic, Map};
    use std::collections::HashMap;

    // Helper function to create a tracker operation metadata entry
    fn make_op(operation: &str) -> Dynamic {
        Dynamic::from(operation.to_string())
    }

    // Helper function to create an integer Dynamic
    fn make_int(value: i64) -> Dynamic {
        Dynamic::from(value)
    }

    // Helper function to create a float Dynamic
    fn make_float(value: f64) -> Dynamic {
        Dynamic::from(value)
    }

    // Helper function to create an array Dynamic
    fn make_array(values: Vec<Dynamic>) -> Dynamic {
        Dynamic::from(values)
    }

    // Helper function to create a map Dynamic
    fn make_map(entries: Vec<(&str, i64)>) -> Dynamic {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert(key.into(), Dynamic::from(value));
        }
        Dynamic::from(map)
    }

    #[test]
    fn test_global_tracker_new() {
        let tracker = GlobalTracker::new();
        // Should be able to create a new tracker without panicking
        assert!(tracker.user_tracked.lock().unwrap().is_empty());
        assert!(tracker.internal_tracked.lock().unwrap().is_empty());
    }

    #[test]
    fn test_merge_worker_state_empty() {
        let tracker = GlobalTracker::new();
        let user_state = HashMap::new();
        let internal_state = HashMap::new();

        let result = tracker.merge_worker_state(user_state, internal_state);
        assert!(result.is_ok());

        assert!(tracker.user_tracked.lock().unwrap().is_empty());
        assert!(tracker.internal_tracked.lock().unwrap().is_empty());
    }

    #[test]
    fn test_merge_worker_state_count_operation() {
        let tracker = GlobalTracker::new();

        // First worker: count = 5
        let mut worker1_user = HashMap::new();
        worker1_user.insert("requests".to_string(), make_int(5));
        worker1_user.insert("__op_requests".to_string(), make_op("count"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Verify first merge
        {
            let global = tracker.user_tracked.lock().unwrap();
            assert_eq!(global.get("requests").unwrap().as_int().unwrap(), 5);
        }

        // Second worker: count = 3
        let mut worker2_user = HashMap::new();
        worker2_user.insert("requests".to_string(), make_int(3));
        worker2_user.insert("__op_requests".to_string(), make_op("count"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify counts are added: 5 + 3 = 8
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(global.get("requests").unwrap().as_int().unwrap(), 8);
    }

    #[test]
    fn test_merge_worker_state_count_with_floats() {
        let tracker = GlobalTracker::new();

        // First worker: count = 5.5
        let mut worker1_user = HashMap::new();
        worker1_user.insert("metric".to_string(), make_float(5.5));
        worker1_user.insert("__op_metric".to_string(), make_op("count"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: count = 3.2
        let mut worker2_user = HashMap::new();
        worker2_user.insert("metric".to_string(), make_float(3.2));
        worker2_user.insert("__op_metric".to_string(), make_op("count"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify counts are added: 5.5 + 3.2 = 8.7
        let global = tracker.user_tracked.lock().unwrap();
        let result = global.get("metric").unwrap().as_float().unwrap();
        assert!((result - 8.7).abs() < 0.001);
    }

    #[test]
    fn test_merge_worker_state_sum_operation() {
        let tracker = GlobalTracker::new();

        // First worker: sum = 100
        let mut worker1_user = HashMap::new();
        worker1_user.insert("total_bytes".to_string(), make_int(100));
        worker1_user.insert("__op_total_bytes".to_string(), make_op("sum"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: sum = 250
        let mut worker2_user = HashMap::new();
        worker2_user.insert("total_bytes".to_string(), make_int(250));
        worker2_user.insert("__op_total_bytes".to_string(), make_op("sum"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify sums are added: 100 + 250 = 350
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(global.get("total_bytes").unwrap().as_int().unwrap(), 350);
    }

    #[test]
    fn test_merge_worker_state_min_operation() {
        let tracker = GlobalTracker::new();

        // First worker: min = 50
        let mut worker1_user = HashMap::new();
        worker1_user.insert("min_latency".to_string(), make_int(50));
        worker1_user.insert("__op_min_latency".to_string(), make_op("min"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: min = 30 (should win)
        let mut worker2_user = HashMap::new();
        worker2_user.insert("min_latency".to_string(), make_int(30));
        worker2_user.insert("__op_min_latency".to_string(), make_op("min"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Third worker: min = 70 (should not win)
        let mut worker3_user = HashMap::new();
        worker3_user.insert("min_latency".to_string(), make_int(70));
        worker3_user.insert("__op_min_latency".to_string(), make_op("min"));
        tracker
            .merge_worker_state(worker3_user, HashMap::new())
            .unwrap();

        // Verify minimum is 30
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(global.get("min_latency").unwrap().as_int().unwrap(), 30);
    }

    #[test]
    fn test_merge_worker_state_max_operation() {
        let tracker = GlobalTracker::new();

        // First worker: max = 50
        let mut worker1_user = HashMap::new();
        worker1_user.insert("max_latency".to_string(), make_int(50));
        worker1_user.insert("__op_max_latency".to_string(), make_op("max"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: max = 30 (should not win)
        let mut worker2_user = HashMap::new();
        worker2_user.insert("max_latency".to_string(), make_int(30));
        worker2_user.insert("__op_max_latency".to_string(), make_op("max"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Third worker: max = 90 (should win)
        let mut worker3_user = HashMap::new();
        worker3_user.insert("max_latency".to_string(), make_int(90));
        worker3_user.insert("__op_max_latency".to_string(), make_op("max"));
        tracker
            .merge_worker_state(worker3_user, HashMap::new())
            .unwrap();

        // Verify maximum is 90
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(global.get("max_latency").unwrap().as_int().unwrap(), 90);
    }

    #[test]
    fn test_merge_worker_state_unique_operation() {
        let tracker = GlobalTracker::new();

        // First worker: unique = ["user1", "user2"]
        let mut worker1_user = HashMap::new();
        worker1_user.insert(
            "unique_users".to_string(),
            make_array(vec![
                Dynamic::from("user1".to_string()),
                Dynamic::from("user2".to_string()),
            ]),
        );
        worker1_user.insert("__op_unique_users".to_string(), make_op("unique"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: unique = ["user2", "user3"] (user2 is duplicate)
        let mut worker2_user = HashMap::new();
        worker2_user.insert(
            "unique_users".to_string(),
            make_array(vec![
                Dynamic::from("user2".to_string()),
                Dynamic::from("user3".to_string()),
            ]),
        );
        worker2_user.insert("__op_unique_users".to_string(), make_op("unique"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify unique values: ["user1", "user2", "user3"]
        let global = tracker.user_tracked.lock().unwrap();
        let result = global
            .get("unique_users")
            .unwrap()
            .clone()
            .into_array()
            .unwrap();
        assert_eq!(result.len(), 3);
        let strings: Vec<String> = result.iter().map(|v| v.to_string()).collect();
        assert!(strings.contains(&"user1".to_string()));
        assert!(strings.contains(&"user2".to_string()));
        assert!(strings.contains(&"user3".to_string()));
    }

    #[test]
    fn test_merge_worker_state_bucket_operation() {
        let tracker = GlobalTracker::new();

        // First worker: bucket = {"200": 5, "404": 2}
        let mut worker1_user = HashMap::new();
        worker1_user.insert(
            "status_codes".to_string(),
            make_map(vec![("200", 5), ("404", 2)]),
        );
        worker1_user.insert("__op_status_codes".to_string(), make_op("bucket"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: bucket = {"200": 3, "500": 1} (200 is duplicate)
        let mut worker2_user = HashMap::new();
        worker2_user.insert(
            "status_codes".to_string(),
            make_map(vec![("200", 3), ("500", 1)]),
        );
        worker2_user.insert("__op_status_codes".to_string(), make_op("bucket"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify buckets are merged: {"200": 8, "404": 2, "500": 1}
        let global = tracker.user_tracked.lock().unwrap();
        let result = global
            .get("status_codes")
            .unwrap()
            .clone()
            .try_cast::<Map>()
            .unwrap();
        assert_eq!(result.get("200").unwrap().as_int().unwrap(), 8);
        assert_eq!(result.get("404").unwrap().as_int().unwrap(), 2);
        assert_eq!(result.get("500").unwrap().as_int().unwrap(), 1);
    }

    #[test]
    fn test_merge_worker_state_error_examples_operation() {
        let tracker = GlobalTracker::new();

        // First worker: error_examples = ["error1", "error2"]
        let mut worker1_internal = HashMap::new();
        worker1_internal.insert(
            "__errors".to_string(),
            make_array(vec![
                Dynamic::from("error1".to_string()),
                Dynamic::from("error2".to_string()),
            ]),
        );
        worker1_internal.insert("__op___errors".to_string(), make_op("error_examples"));
        tracker
            .merge_worker_state(HashMap::new(), worker1_internal)
            .unwrap();

        // Second worker: error_examples = ["error3", "error4"]
        // Should merge but limit to 3 total
        let mut worker2_internal = HashMap::new();
        worker2_internal.insert(
            "__errors".to_string(),
            make_array(vec![
                Dynamic::from("error3".to_string()),
                Dynamic::from("error4".to_string()),
            ]),
        );
        worker2_internal.insert("__op___errors".to_string(), make_op("error_examples"));
        tracker
            .merge_worker_state(HashMap::new(), worker2_internal)
            .unwrap();

        // Verify error examples are limited to 3
        let global = tracker.internal_tracked.lock().unwrap();
        let result = global
            .get("__errors")
            .unwrap()
            .clone()
            .into_array()
            .unwrap();
        assert!(result.len() <= 3);
    }

    #[test]
    fn test_merge_worker_state_replace_operation() {
        let tracker = GlobalTracker::new();

        // First worker: value = "first"
        let mut worker1_user = HashMap::new();
        worker1_user.insert("last_seen".to_string(), Dynamic::from("first".to_string()));
        worker1_user.insert("__op_last_seen".to_string(), make_op("replace"));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker: value = "second" (should replace)
        let mut worker2_user = HashMap::new();
        worker2_user.insert("last_seen".to_string(), Dynamic::from("second".to_string()));
        worker2_user.insert("__op_last_seen".to_string(), make_op("replace"));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify value is replaced
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(
            global.get("last_seen").unwrap().to_string(),
            "second".to_string()
        );
    }

    #[test]
    fn test_merge_worker_state_no_operation_metadata() {
        let tracker = GlobalTracker::new();

        // Worker without operation metadata (should default to replace)
        let mut worker1_user = HashMap::new();
        worker1_user.insert("value".to_string(), make_int(42));
        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Second worker
        let mut worker2_user = HashMap::new();
        worker2_user.insert("value".to_string(), make_int(99));
        tracker
            .merge_worker_state(worker2_user, HashMap::new())
            .unwrap();

        // Verify last value wins (replace behavior)
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(global.get("value").unwrap().as_int().unwrap(), 99);
    }

    #[test]
    fn test_merge_worker_stats_basic() {
        let tracker = GlobalTracker::new();

        let worker1_stats = ProcessingStats {
            lines_errors: 5,
            errors: 5,
            files_processed: 2,
            script_executions: 100,
            ..Default::default()
        };

        tracker.merge_worker_stats(&worker1_stats).unwrap();

        let global = tracker.processing_stats.lock().unwrap();
        assert_eq!(global.lines_errors, 5);
        assert_eq!(global.errors, 5);
        assert_eq!(global.files_processed, 2);
        assert_eq!(global.script_executions, 100);
    }

    #[test]
    fn test_merge_worker_stats_multiple_workers() {
        let tracker = GlobalTracker::new();

        let worker1_stats = ProcessingStats {
            lines_errors: 3,
            files_processed: 1,
            script_executions: 50,
            ..Default::default()
        };
        tracker.merge_worker_stats(&worker1_stats).unwrap();

        let worker2_stats = ProcessingStats {
            lines_errors: 2,
            files_processed: 1,
            script_executions: 75,
            ..Default::default()
        };
        tracker.merge_worker_stats(&worker2_stats).unwrap();

        let global = tracker.processing_stats.lock().unwrap();
        assert_eq!(global.lines_errors, 5); // 3 + 2
        assert_eq!(global.files_processed, 2); // 1 + 1
        assert_eq!(global.script_executions, 125); // 50 + 75
    }

    #[test]
    fn test_merge_worker_stats_timestamp_fields() {
        let tracker = GlobalTracker::new();

        let worker1_stats = ProcessingStats {
            timestamp_detected_events: 10,
            timestamp_parsed_events: 8,
            timestamp_absent_events: 2,
            ..Default::default()
        };
        tracker.merge_worker_stats(&worker1_stats).unwrap();

        let worker2_stats = ProcessingStats {
            timestamp_detected_events: 15,
            timestamp_parsed_events: 12,
            timestamp_absent_events: 3,
            ..Default::default()
        };
        tracker.merge_worker_stats(&worker2_stats).unwrap();

        let global = tracker.processing_stats.lock().unwrap();
        assert_eq!(global.timestamp_detected_events, 25); // 10 + 15
        assert_eq!(global.timestamp_parsed_events, 20); // 8 + 12
        assert_eq!(global.timestamp_absent_events, 5); // 2 + 3
    }

    #[test]
    fn test_merge_worker_stats_lines_read_not_merged() {
        let tracker = GlobalTracker::new();

        let worker1_stats = ProcessingStats {
            lines_read: 100, // This should NOT be merged
            lines_errors: 5,
            ..Default::default()
        };
        tracker.merge_worker_stats(&worker1_stats).unwrap();

        let worker2_stats = ProcessingStats {
            lines_read: 200, // This should NOT be merged
            lines_errors: 3,
            ..Default::default()
        };
        tracker.merge_worker_stats(&worker2_stats).unwrap();

        let global = tracker.processing_stats.lock().unwrap();
        // lines_read should not be merged (remains at default 0)
        assert_eq!(global.lines_read, 0);
        // But other stats should be merged
        assert_eq!(global.lines_errors, 8); // 5 + 3
    }

    #[test]
    fn test_global_tracker_multiple_keys() {
        let tracker = GlobalTracker::new();

        // Worker with multiple tracked values
        let mut worker1_user = HashMap::new();
        worker1_user.insert("count1".to_string(), make_int(10));
        worker1_user.insert("__op_count1".to_string(), make_op("count"));
        worker1_user.insert("count2".to_string(), make_int(20));
        worker1_user.insert("__op_count2".to_string(), make_op("count"));
        worker1_user.insert("max_value".to_string(), make_int(100));
        worker1_user.insert("__op_max_value".to_string(), make_op("max"));

        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        // Verify all values are tracked
        let global = tracker.user_tracked.lock().unwrap();
        assert_eq!(global.get("count1").unwrap().as_int().unwrap(), 10);
        assert_eq!(global.get("count2").unwrap().as_int().unwrap(), 20);
        assert_eq!(global.get("max_value").unwrap().as_int().unwrap(), 100);
    }

    #[test]
    fn test_merge_worker_state_internal_metadata_copied() {
        let tracker = GlobalTracker::new();

        // Internal state should copy metadata
        let mut worker1_internal = HashMap::new();
        worker1_internal.insert("__errors".to_string(), make_array(vec![]));
        worker1_internal.insert("__op___errors".to_string(), make_op("error_examples"));

        tracker
            .merge_worker_state(HashMap::new(), worker1_internal)
            .unwrap();

        let global = tracker.internal_tracked.lock().unwrap();
        // Metadata should be copied for internal state
        assert!(global.contains_key("__op___errors"));
    }

    #[test]
    fn test_merge_worker_state_user_metadata_not_copied() {
        let tracker = GlobalTracker::new();

        // User state should NOT copy metadata (it's looked up from worker state)
        let mut worker1_user = HashMap::new();
        worker1_user.insert("count".to_string(), make_int(5));
        worker1_user.insert("__op_count".to_string(), make_op("count"));

        tracker
            .merge_worker_state(worker1_user, HashMap::new())
            .unwrap();

        let global = tracker.user_tracked.lock().unwrap();
        // Metadata should NOT be in global user state
        assert!(!global.contains_key("__op_count"));
        // But the value should be there
        assert!(global.contains_key("count"));
    }
}
