//! JWT (JSON Web Token) parsing functions for Rhai scripts.
//!
//! Provides functions for parsing and extracting JWT token components.

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use rhai::{Dynamic, Engine, Map};

/// Maximum length for parsed inputs (1MB)
const MAX_PARSE_LEN: usize = 1_048_576;

fn is_base64url_char(ch: char) -> bool {
    matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '=')
}

fn decode_jwt_segment(segment: &str) -> Option<Vec<u8>> {
    if segment.len() > MAX_PARSE_LEN {
        return None;
    }
    match URL_SAFE_NO_PAD.decode(segment.as_bytes()) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            let mut padded = segment.to_string();
            // Base64 requires lengths in multiples of 4; older Rust on OpenBSD lacks is_multiple_of.
            #[allow(unknown_lints, clippy::manual_is_multiple_of)]
            while padded.len() % 4 != 0 {
                padded.push('=');
                if padded.len() > MAX_PARSE_LEN {
                    return None;
                }
            }
            URL_SAFE.decode(padded.as_bytes()).ok()
        }
    }
}

fn jwt_segment_to_map(segment: &str) -> Map {
    if let Some(bytes) = decode_jwt_segment(segment) {
        if bytes.len() <= MAX_PARSE_LEN {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                let dynamic = crate::event::json_to_dynamic(&json);
                if let Some(map) = dynamic.try_cast::<Map>() {
                    return map;
                }
            }
        }
    }
    Map::new()
}

fn parse_jwt_impl(input: &str) -> Map {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return Map::new();
    }

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return Map::new();
    }

    if parts[0].is_empty() || parts[1].is_empty() {
        return Map::new();
    }

    if !parts[0].chars().all(is_base64url_char) || !parts[1].chars().all(is_base64url_char) {
        return Map::new();
    }

    let header_map = jwt_segment_to_map(parts[0]);
    let claims_map = jwt_segment_to_map(parts[1]);

    let signature_segment = if parts.len() == 3 { parts[2] } else { "" };

    let mut result = Map::new();
    result.insert("header".into(), Dynamic::from(header_map.clone()));
    result.insert("claims".into(), Dynamic::from(claims_map.clone()));
    result.insert(
        "signature_b64u".into(),
        Dynamic::from(signature_segment.to_string()),
    );

    if let Some(alg) = header_map
        .get("alg")
        .and_then(|v| v.clone().into_string().ok())
    {
        result.insert("alg".into(), Dynamic::from(alg));
    }
    if let Some(kid) = header_map
        .get("kid")
        .and_then(|v| v.clone().into_string().ok())
    {
        result.insert("kid".into(), Dynamic::from(kid));
    }
    if let Some(typ) = header_map
        .get("typ")
        .and_then(|v| v.clone().into_string().ok())
    {
        result.insert("typ".into(), Dynamic::from(typ));
    }

    result
}

/// Register JWT parsing functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("parse_jwt", parse_jwt_impl);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Scope;

    #[test]
    fn test_parse_jwt_function() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "jwt",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
             eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiYWRtaW4iOnRydWV9.\
             SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        );

        let result: Map = engine
            .eval_with_scope(&mut scope, r#"parse_jwt(jwt)"#)
            .unwrap();

        assert_eq!(
            result.get("alg").unwrap().clone().into_string().unwrap(),
            "HS256"
        );
        assert_eq!(
            result.get("typ").unwrap().clone().into_string().unwrap(),
            "JWT"
        );
        assert_eq!(
            result
                .get("signature_b64u")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );

        let claims = result
            .get("claims")
            .unwrap()
            .clone()
            .try_cast::<Map>()
            .unwrap();
        assert_eq!(
            claims.get("sub").unwrap().clone().into_string().unwrap(),
            "1234567890"
        );
        assert!(claims.get("admin").unwrap().clone().as_bool().unwrap());

        scope.push("invalid_jwt", "ab$cd.efg");
        let invalid: Map = engine
            .eval_with_scope(&mut scope, r#"parse_jwt(invalid_jwt)"#)
            .unwrap();
        assert!(invalid.is_empty());
    }
}
