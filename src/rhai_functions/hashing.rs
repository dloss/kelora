//! Hashing and pseudonymization helpers for Rhai scripts.
//!
//! Includes fast hashes, cryptographic digests, and salted pseudonyms.

use argon2::password_hash::{Salt, SaltString};
use argon2::{Argon2, PasswordHasher};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rhai::Engine;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::{Mutex, RwLock};
use xxhash_rust::xxh3::xxh3_64;

type HmacSha256 = Hmac<Sha256>;

/// Runtime configuration for hashing module
#[derive(Debug, Clone, Default)]
pub struct HashingRuntimeConfig {
    pub verbose: u8,
    pub use_emoji: bool,
    pub quiet_level: u8,
}

static RUNTIME_CONFIG: LazyLock<RwLock<HashingRuntimeConfig>> =
    LazyLock::new(|| RwLock::new(HashingRuntimeConfig::default()));

/// Set runtime configuration for hashing functions
pub fn set_runtime_config(config: HashingRuntimeConfig) {
    let mut guard = RUNTIME_CONFIG
        .write()
        .expect("hashing runtime config poisoned");
    *guard = config;
}

/// Log pseudonym initialization (only on verbose level 2+)
fn log_pseudonym_init(message: &str) {
    let config = RUNTIME_CONFIG
        .read()
        .expect("hashing runtime config poisoned");
    if config.verbose >= 2 {
        let prefix = if config.use_emoji { "🔹" } else { "kelora:" };
        eprintln!("{} {}", prefix, message);
    }
}

/// Warn that pseudonyms are using an ephemeral key (shown at default verbosity).
///
/// Unlike `log_pseudonym_init`, this safety warning is shown without `-vv` because
/// the failure mode is silent: pseudonyms won't correlate across separate runs.
/// Suppressed when diagnostics are off (`--silent`/`--no-diagnostics`, quiet_level >= 1).
fn warn_pseudonym_ephemeral() {
    let config = RUNTIME_CONFIG
        .read()
        .expect("hashing runtime config poisoned");
    if config.quiet_level == 0 {
        let prefix = if config.use_emoji {
            "⚠️"
        } else {
            "kelora:"
        };
        eprintln!(
            "{} KELORA_SECRET not set; using ephemeral key, pseudonyms won't be stable across runs",
            prefix
        );
    }
}

/// Master key for pseudonymization (derived once at startup)
static MASTER_KEY: LazyLock<MasterKeyState> = LazyLock::new(|| {
    match std::env::var("KELORA_SECRET") {
        Ok(secret) if !secret.is_empty() => {
            match derive_master_key_from_secret(&secret) {
                Ok(key) => {
                    log_pseudonym_init("pseudonym: ON (stable; KELORA_SECRET)");
                    MasterKeyState::Stable(key)
                }
                Err(e) => {
                    // Always show fatal errors
                    eprintln!("kelora: pseudonym init failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Ok(_) => {
            // Always show fatal errors
            eprintln!("kelora: KELORA_SECRET must not be empty");
            std::process::exit(1);
        }
        Err(_) => {
            // Generate ephemeral key
            let mut key = [0u8; 32];
            for byte in &mut key {
                *byte = fastrand::u8(..);
            }
            warn_pseudonym_ephemeral();
            log_pseudonym_init("pseudonym: ON (ephemeral; not stable)");
            MasterKeyState::Ephemeral(key)
        }
    }
});

/// Domain-specific derived keys (cached)
static DOMAIN_KEYS: LazyLock<Mutex<HashMap<String, [u8; 32]>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

enum MasterKeyState {
    Stable([u8; 32]),
    Ephemeral([u8; 32]),
}

impl MasterKeyState {
    fn as_bytes(&self) -> &[u8; 32] {
        match self {
            MasterKeyState::Stable(k) => k,
            MasterKeyState::Ephemeral(k) => k,
        }
    }
}

/// Derive master key from secret using Argon2id
fn derive_master_key_from_secret(secret: &str) -> Result<[u8; 32], String> {
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(
            64 * 1024, // 64 MiB
            3,         // iterations
            1,         // parallelism
            Some(32),  // output length
        )
        .map_err(|e| format!("Argon2 params error: {}", e))?,
    );

    // Use fixed salt "kelora:v1:master"
    let salt = SaltString::encode_b64(b"kelora:v1:master")
        .map_err(|e| format!("Salt encoding error: {}", e))?;

    let hash = argon2
        .hash_password(secret.as_bytes(), Salt::try_from(salt.as_str()).unwrap())
        .map_err(|e| format!("Argon2 hashing error: {}", e))?;

    let hash_bytes = hash
        .hash
        .ok_or_else(|| "Argon2 produced no hash".to_string())?;

    let mut key = [0u8; 32];
    key.copy_from_slice(hash_bytes.as_bytes());
    Ok(key)
}

/// Derive domain-specific key using HKDF-SHA256
fn derive_domain_key(domain: &str) -> Result<[u8; 32], String> {
    // Check cache first
    {
        let cache = DOMAIN_KEYS.lock().unwrap();
        if let Some(key) = cache.get(domain) {
            return Ok(*key);
        }
    }

    let master = MASTER_KEY.as_bytes();
    let info = format!("kelora:v1:{}", domain);

    let hkdf = Hkdf::<Sha256>::new(None, master);
    let mut okm = [0u8; 32];
    hkdf.expand(info.as_bytes(), &mut okm)
        .map_err(|e| format!("HKDF expansion error: {}", e))?;

    // Cache the derived key
    {
        let mut cache = DOMAIN_KEYS.lock().unwrap();
        cache.insert(domain.to_string(), okm);
    }

    Ok(okm)
}

/// Generate pseudonym token using HMAC-SHA256
fn pseudonym_impl(value: &str, domain: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    if domain.is_empty() {
        return Err("pseudonym: domain must be non-empty".into());
    }

    // Force initialization of master key (triggers logging)
    let _ = MASTER_KEY.as_bytes();

    let domain_key = derive_domain_key(domain)
        .map_err(|e| format!("pseudonym: domain key derivation failed: {}", e))?;

    // HMAC-SHA256(key=domain_key, data=domain || value)
    let mut mac =
        HmacSha256::new_from_slice(&domain_key).map_err(|e| format!("HMAC init error: {}", e))?;

    mac.update(domain.as_bytes());
    mac.update(value.as_bytes());

    let result = mac.finalize();
    let tag = result.into_bytes();

    // base64url encode (unpadded) and take first 24 chars
    let encoded = URL_SAFE_NO_PAD.encode(tag);
    Ok(encoded[..24].to_string())
}

/// Fast non-cryptographic hash for bucketing/sampling
/// Uses xxh3_64 for performance
fn bucket_impl(value: &str) -> i64 {
    xxh3_64(value.as_bytes()) as i64
}

/// Apply a named hash algorithm to input
/// Supported: "sha256" (default), "xxh3"
fn hash_impl(value: &str, algo: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    let algo_lower = algo.to_lowercase();
    match algo_lower.as_str() {
        "sha256" => {
            let mut hasher = Sha256::new();
            hasher.update(value.as_bytes());
            Ok(hex::encode(hasher.finalize()))
        }
        "xxh3" => {
            let hash = xxh3_64(value.as_bytes());
            Ok(format!("{:016x}", hash))
        }
        _ => Err(format!("Unknown hash algorithm '{}'. Supported: sha256, xxh3", algo).into()),
    }
}

/// Wrapper for hash with default algorithm
fn hash_default_impl(value: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    hash_impl(value, "sha256")
}

/// Register hashing functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // bucket() - fast non-cryptographic hash for bucketing/sampling
    engine.register_fn("bucket", bucket_impl);

    // hash() - multi-algorithm hashing
    engine.register_fn("hash", hash_default_impl);
    engine.register_fn("hash", hash_impl);

    // pseudonym() - domain-separated pseudonymization
    engine.register_fn("pseudonym", pseudonym_impl);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket() {
        let result1 = bucket_impl("test");
        let result2 = bucket_impl("test");
        let result3 = bucket_impl("other");

        // Same input should produce same hash
        assert_eq!(result1, result2);
        // Different input should (probably) produce different hash
        assert_ne!(result1, result3);
    }

    #[test]
    fn test_hash_sha256() {
        let result = hash_impl("hello", "sha256").unwrap();
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_hash_xxh3() {
        let result = hash_impl("hello", "xxh3").unwrap();
        // xxh3 is deterministic, just verify it's a valid hex string
        assert_eq!(result.len(), 16);
        assert!(result.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_unknown_algo() {
        let result = hash_impl("hello", "unknown");
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_default() {
        let result = hash_default_impl("hello").unwrap();
        // Should default to sha256
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_pseudonym_empty_domain() {
        let result = pseudonym_impl("value", "");
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("domain must be non-empty"));
    }

    #[test]
    fn test_pseudonym_deterministic() {
        // Same value and domain should produce same token
        let result1 = pseudonym_impl("user123", "kelora:v1:email").unwrap();
        let result2 = pseudonym_impl("user123", "kelora:v1:email").unwrap();
        assert_eq!(result1, result2);
        assert_eq!(result1.len(), 24);
    }

    #[test]
    fn test_pseudonym_domain_separation() {
        // Same value, different domains should produce different tokens
        let result1 = pseudonym_impl("user123", "kelora:v1:email").unwrap();
        let result2 = pseudonym_impl("user123", "kelora:v1:ip").unwrap();
        assert_ne!(result1, result2);
    }

    #[test]
    fn test_pseudonym_different_values() {
        // Different values, same domain should produce different tokens
        let result1 = pseudonym_impl("user123", "kelora:v1:email").unwrap();
        let result2 = pseudonym_impl("user456", "kelora:v1:email").unwrap();
        assert_ne!(result1, result2);
    }

    #[test]
    fn test_pseudonym_output_format() {
        let result = pseudonym_impl("test", "kelora:v1:test").unwrap();
        // Should be exactly 24 characters
        assert_eq!(result.len(), 24);
        // Should only contain base64url characters (no padding)
        assert!(result
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(!result.contains('='));
    }

    #[test]
    fn test_rhai_integration() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test bucket
        let result: i64 = engine.eval(r#"bucket("test")"#).unwrap();
        assert_eq!(result, bucket_impl("test"));

        // Test hash with default
        let result: String = engine.eval(r#"hash("hello")"#).unwrap();
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );

        // Test hash with algo
        let result: String = engine.eval(r#"hash("hello", "xxh3")"#).unwrap();
        assert_eq!(result.len(), 16);

        // Test pseudonym
        let result: String = engine
            .eval(r#"pseudonym("user123", "kelora:v1:email")"#)
            .unwrap();
        assert_eq!(result.len(), 24);

        // Test pseudonym with empty domain
        let result = engine.eval::<String>(r#"pseudonym("user123", "")"#);
        assert!(result.is_err());
    }
}
