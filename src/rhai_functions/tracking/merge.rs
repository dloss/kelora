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

pub(super) fn record_operation_metadata(key: &str, operation: &str) {
    with_internal_tracking(|internal| {
        internal.insert(
            format!("__op_{}", key),
            Dynamic::from(operation.to_string()),
        );
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
