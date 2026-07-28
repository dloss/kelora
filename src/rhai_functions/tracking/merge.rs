use super::with_internal_tracking;
use hyperloglog::HyperLogLog;
use rhai::Dynamic;
use tdigests::TDigest;

/// Default error rate for HyperLogLog (~1.04% standard error)
/// This corresponds to 2^14 = 16384 registers, using ~12KB of memory
const HLL_DEFAULT_ERROR_RATE: f64 = 0.01;

/// Fixed seed for HyperLogLog to ensure deterministic hashing across instances
/// This is required for merging HLLs from different workers in parallel mode
const HLL_SEED: u128 = 0x6b656c6f72615f686c6c5f73656564; // "kelora_hll_seed" in hex

/// Magic bytes to identify HLL blobs (distinguishes from t-digest blobs)
const HLL_MAGIC: &[u8; 4] = b"HLL\x01";

/// Public function name(s) behind an internal operation id, for error messages.
pub(crate) fn op_display_name(op: &str) -> &str {
    match op {
        "sum" => "track_sum",
        "count" => "track_stats (count)",
        "avg" => "track_avg",
        "min" => "track_min",
        "max" => "track_max",
        "unique" => "track_unique",
        // Internal op id predates the 2.0 renames (track_bucket → track_freq).
        "bucket" => "track_freq",
        "cardinality" => "track_cardinality",
        "percentiles" => "track_percentiles",
        "top" => "track_top",
        "bottom" => "track_bottom",
        "top_by" => "track_top_by",
        "bottom_by" => "track_bottom_by",
        other => other,
    }
}

/// Record which track operation owns a metric key, erroring if a different
/// operation already uses the same key. Without this check the per-key merge
/// strategy (parallel workers, span windows) silently blends incompatible
/// shapes into garbage.
pub(super) fn ensure_operation_metadata(
    key: &str,
    operation: &str,
) -> Result<(), Box<rhai::EvalAltResult>> {
    with_internal_tracking(|internal| {
        let op_key = format!("__op_{}", key);
        if let Some(existing) = internal.get(&op_key) {
            // Refcount-bump comparison; this runs per track_* call per event,
            // so avoid deep-copying the stored op string.
            let existing_op = existing.clone().into_immutable_string().unwrap_or_default();
            if existing_op != operation {
                return Err(format!(
                    "metric '{}' is already tracked by {}; each metric name can be used by only one track function (use a different name for {})",
                    key,
                    op_display_name(&existing_op),
                    op_display_name(operation)
                )
                .into());
            }
        } else {
            internal.insert(op_key, Dynamic::from(operation.to_string()));
        }
        Ok(())
    })
}

/// Count a skipped Unit `()` value for a metric so `--diagnostics` can surface
/// it at the end of the run (a high skip count usually means a field-name typo).
pub(super) fn record_skipped_unit(key: &str) {
    with_internal_tracking(|internal| {
        let skip_key = format!("__kelora_track_skipped_{}", key);
        if let Some(existing) = internal.get_mut(&skip_key) {
            // Steady-state path (a typo'd field skips on every event):
            // increment in place, no further allocation.
            let current = existing.as_int().unwrap_or(0);
            *existing = Dynamic::from(current + 1);
        } else {
            // Additive merge across parallel workers.
            internal.insert(format!("__op_{}", skip_key), Dynamic::from("count"));
            internal.insert(skip_key, Dynamic::from(1_i64));
        }
    });
}

pub(super) fn merge_numeric(existing: Option<Dynamic>, new_value: Dynamic) -> Dynamic {
    let new_is_float = new_value.is_float();

    if let Some(current) = existing {
        let current_is_float = current.is_float();

        if current_is_float || new_is_float {
            let current_total = if current_is_float {
                current.as_float().unwrap_or(0.0)
            } else {
                current.as_int().unwrap_or(0) as f64
            };

            let incoming = if new_is_float {
                new_value.as_float().unwrap_or(0.0)
            } else {
                new_value.as_int().unwrap_or(0) as f64
            };

            Dynamic::from(current_total + incoming)
        } else {
            let current_total = current.as_int().unwrap_or(0);
            let incoming = new_value.as_int().unwrap_or(0);
            Dynamic::from(current_total + incoming)
        }
    } else {
        new_value
    }
}

/// Centroid budget for a stored t-digest.
///
/// `TDigest::merge` only concatenates and sorts centroid lists — it never
/// combines them — so without an explicit `compress` the digest gains one
/// centroid per tracked value and every subsequent event pays an O(n)
/// deserialize/sort/serialize round trip. That made `track_percentiles`,
/// `track_stats` and `--describe` quadratic in event count (#377).
///
/// 100 centroids keeps the estimator's error around 0.3% on realistic latency
/// distributions — far inside the ~1% a t-digest promises — while bounding the
/// stored blob at ~3KB (it is allowed to reach twice the budget between
/// compressions; see [`compress_tdigest`]).
pub(crate) const TDIGEST_MAX_CENTROIDS: usize = 100;

/// Compress a digest back to [`TDIGEST_MAX_CENTROIDS`], but only once it has
/// grown to twice that.
///
/// Compressing on every event would work, yet it re-merges the whole centroid
/// list each time; letting the digest run to 2x and then halving it amortizes
/// that cost over `TDIGEST_MAX_CENTROIDS` events and keeps the per-event work
/// genuinely constant.
pub(crate) fn compress_tdigest(digest: &mut TDigest) {
    if digest.centroids().len() > TDIGEST_MAX_CENTROIDS * 2 {
        digest.compress(TDIGEST_MAX_CENTROIDS);
    }
}

/// Helper function to serialize a TDigest to bytes for storage in Dynamic
/// We store centroids as the serialization format
pub(super) fn serialize_tdigest(digest: &TDigest) -> Vec<u8> {
    let centroids = digest.centroids();
    let mut bytes = Vec::new();

    let count = centroids.len();
    bytes.extend_from_slice(&count.to_le_bytes());

    for centroid in centroids {
        bytes.extend_from_slice(&centroid.mean.to_le_bytes());
        bytes.extend_from_slice(&centroid.weight.to_le_bytes());
    }

    bytes
}

/// Helper function to deserialize a TDigest from bytes stored in Dynamic
pub(super) fn deserialize_tdigest(bytes: &[u8]) -> Option<TDigest> {
    if bytes.len() < 8 {
        return None;
    }

    let count = usize::from_le_bytes(bytes[0..8].try_into().ok()?);

    if bytes.len() < 8 + count * 16 {
        return None;
    }

    let mut centroids = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 8 + i * 16;
        let mean = f64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
        let weight = f64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().ok()?);
        centroids.push(tdigests::Centroid::new(mean, weight));
    }

    Some(TDigest::from_centroids(centroids))
}

/// Helper function to serialize a HyperLogLog to bytes for storage in Dynamic
/// Uses serde with bincode-style format
pub(super) fn serialize_hll(hll: &HyperLogLog) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(HLL_MAGIC);

    if let Ok(json) = serde_json::to_vec(hll) {
        bytes.extend_from_slice(&json);
    }

    bytes
}

/// Helper function to deserialize a HyperLogLog from bytes stored in Dynamic
pub(super) fn deserialize_hll(bytes: &[u8]) -> Option<HyperLogLog> {
    if bytes.len() < 4 || &bytes[0..4] != HLL_MAGIC {
        return None;
    }

    serde_json::from_slice(&bytes[4..]).ok()
}

/// Check if a blob is an HLL (vs t-digest or other)
pub(super) fn is_hll_blob(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[0..4] == HLL_MAGIC
}

/// Create a new HyperLogLog with the default error rate and fixed seed
pub(super) fn new_hll() -> HyperLogLog {
    HyperLogLog::new_deterministic(HLL_DEFAULT_ERROR_RATE, HLL_SEED)
}

/// Create a new HyperLogLog with a custom error rate and fixed seed
pub(super) fn new_hll_with_error(error_rate: f64) -> HyperLogLog {
    HyperLogLog::new_deterministic(error_rate, HLL_SEED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_numeric_integers() {
        let result = merge_numeric(Some(Dynamic::from(5i64)), Dynamic::from(3i64));
        assert_eq!(result.as_int().unwrap(), 8);
    }

    #[test]
    fn test_merge_numeric_floats() {
        let result = merge_numeric(Some(Dynamic::from(5.5f64)), Dynamic::from(3.2f64));
        let value = result.as_float().unwrap();
        assert!((value - 8.7).abs() < 0.001);
    }

    #[test]
    fn test_merge_numeric_mixed_int_and_float() {
        let result = merge_numeric(Some(Dynamic::from(5i64)), Dynamic::from(3.5f64));
        let value = result.as_float().unwrap();
        assert!((value - 8.5).abs() < 0.001);
    }

    #[test]
    fn test_merge_numeric_no_existing() {
        let result = merge_numeric(None, Dynamic::from(42i64));
        assert_eq!(result.as_int().unwrap(), 42);
    }

    #[test]
    fn test_merge_numeric_edge_case_zero_plus_zero() {
        let result = merge_numeric(Some(Dynamic::from(0i64)), Dynamic::from(0i64));
        assert_eq!(result.as_int().unwrap(), 0);
    }

    #[test]
    fn test_merge_numeric_edge_case_negative_numbers() {
        let result = merge_numeric(Some(Dynamic::from(-5i64)), Dynamic::from(-3i64));
        assert_eq!(result.as_int().unwrap(), -8);
    }

    #[test]
    fn test_merge_numeric_edge_case_large_integers() {
        let result = merge_numeric(
            Some(Dynamic::from(1_000_000_000i64)),
            Dynamic::from(2_000_000_000i64),
        );
        assert_eq!(result.as_int().unwrap(), 3_000_000_000i64);
    }

    /// #377: `TDigest::merge` never combines centroids, so a digest fed one
    /// value at a time grew a centroid per event and made every later event
    /// more expensive. The stored digest must stay bounded no matter how many
    /// values it has seen.
    #[test]
    fn test_tdigest_centroids_stay_bounded() {
        let mut digest = TDigest::from_values(vec![0.0]);
        for i in 1..10_000 {
            digest = digest.merge(&TDigest::from_values(vec![(i % 997) as f64]));
            compress_tdigest(&mut digest);
            assert!(
                digest.centroids().len() <= TDIGEST_MAX_CENTROIDS * 2 + 1,
                "digest grew to {} centroids after {} values",
                digest.centroids().len(),
                i + 1
            );
        }
        // Serialized size follows the centroid count, so the blob rewritten on
        // every event stays small too.
        assert!(serialize_tdigest(&digest).len() <= 8 + 16 * (TDIGEST_MAX_CENTROIDS * 2 + 1));
    }

    /// Compressing is only worth it if the estimates survive it. Uniform
    /// 0..=9999 has percentiles we can state exactly; a 100-centroid digest
    /// should land within 1% of them — the error bar t-digest advertises.
    #[test]
    fn test_compressed_tdigest_stays_accurate() {
        let mut digest = TDigest::from_values(vec![0.0]);
        for i in 1..10_000 {
            digest = digest.merge(&TDigest::from_values(vec![i as f64]));
            compress_tdigest(&mut digest);
        }

        for (q, expected) in [(0.5, 4999.5), (0.95, 9499.05), (0.99, 9899.01)] {
            let estimate = digest.estimate_quantile(q);
            let error = (estimate - expected).abs() / expected;
            assert!(
                error < 0.01,
                "p{} estimated {estimate}, expected ~{expected} ({:.2}% off)",
                q * 100.0,
                error * 100.0
            );
        }
    }

    #[test]
    fn test_hll_serialization_roundtrip() {
        let mut hll = new_hll();
        hll.insert(&"user1");
        hll.insert(&"user2");
        hll.insert(&"user3");

        let bytes = serialize_hll(&hll);
        assert!(is_hll_blob(&bytes));

        let restored = deserialize_hll(&bytes).unwrap();
        assert_eq!(restored.len(), hll.len());
    }
}
