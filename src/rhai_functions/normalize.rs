use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use rhai::{Dynamic, Engine, Map};
use std::collections::HashMap;

/// Luhn algorithm validation for credit card numbers
#[allow(clippy::manual_is_multiple_of)]
fn is_valid_luhn(digits: &str) -> bool {
    let digits: Vec<u32> = digits
        .chars()
        .filter(|c| c.is_ascii_digit())
        .filter_map(|c| c.to_digit(10))
        .collect();

    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }

    // Reject all-zeros (would pass Luhn but isn't a real card)
    if digits.iter().all(|&d| d == 0) {
        return false;
    }

    // Luhn checksum
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    doubled - 9
                } else {
                    doubled
                }
            } else {
                d
            }
        })
        .sum();

    sum % 10 == 0
}

/// Pattern name to regex mapping for normalization
static PATTERNS: Lazy<HashMap<&'static str, Vec<Regex>>> = Lazy::new(|| {
    let mut map = HashMap::new();

    // IPv4 address with proper octet validation - using word boundaries
    let octet = r"(?:25[0-5]|2[0-4]\d|1\d{2}|[1-9]?\d)";
    map.insert(
        "ipv4",
        vec![Regex::new(&format!(r"\b{octet}\.{octet}\.{octet}\.{octet}\b")).unwrap()],
    );

    // IPv4 with port - using word boundaries
    map.insert(
        "ipv4_port",
        vec![Regex::new(&format!(
            r"\b{octet}\.{octet}\.{octet}\.{octet}:(?:0|[1-9]\d{{0,3}}|[1-5]\d{{4}}|6[0-4]\d{{3}}|65[0-4]\d{{2}}|655[0-2]\d|6553[0-5])\b"
        ))
        .unwrap()],
    );

    // IPv6 (simplified - matches common IPv6 patterns with word boundaries)
    map.insert(
        "ipv6",
        vec![Regex::new(
            r"(?i)\b(?:[0-9A-Fa-f]{1,4}:){7}[0-9A-Fa-f]{1,4}\b|(?:[0-9A-Fa-f]{1,4}:){1,6}:[0-9A-Fa-f]{1,4}|(?:[0-9A-Fa-f]{1,4}:){1,5}(?::[0-9A-Fa-f]{1,4}){1,2}|fe80:(?::[0-9A-Fa-f]{0,4}){0,4}%[0-9A-Za-z]{1,}|::(?:ffff:)?(?:(?:25[0-5]|2[0-4]\d|1?\d?\d)\.){3}(?:25[0-5]|2[0-4]\d|1?\d?\d)"
        )
        .unwrap()],
    );

    // Email address
    map.insert(
        "email",
        vec![Regex::new(r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b").unwrap()],
    );

    // URL with protocol
    map.insert(
        "url",
        vec![Regex::new(
            r"\b(?:[a-z][a-z0-9+.-]*):\/\/(?:(?:[^\s:@]+(?::[^\s:@]*)?@)?(?:[^\s:/?#]+)(?::\d+)?(?:\/[^\s?#]*)?(?:\?[^\s#]*)?(?:#[^\s]*)?)\b"
        )
        .unwrap()],
    );

    // FQDN (Fully Qualified Domain Name)
    map.insert(
        "fqdn",
        vec![
            Regex::new(r"\b(?:[a-z](?:[a-z0-9-]{0,63}[a-z0-9])?\.){2,}[a-z0-9][a-z0-9-]{0,8}\b")
                .unwrap(),
        ],
    );

    // UUID
    map.insert(
        "uuid",
        vec![Regex::new(
            r"\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b",
        )
        .unwrap()],
    );

    // MAC address (colon or dot separated)
    map.insert(
        "mac",
        vec![
            Regex::new(r"\b(?:[0-9A-Fa-f]{2}[:-]){5}[0-9A-Fa-f]{2}\b").unwrap(),
            Regex::new(r"\b(?:[0-9A-Fa-f]{4}\.){2}[0-9A-Fa-f]{4}\b").unwrap(),
        ],
    );

    // Hash values
    map.insert("md5", vec![Regex::new(r"\b[a-fA-F0-9]{32}\b").unwrap()]);
    map.insert("sha1", vec![Regex::new(r"\b[a-fA-F0-9]{40}\b").unwrap()]);
    map.insert("sha256", vec![Regex::new(r"\b[a-fA-F0-9]{64}\b").unwrap()]);

    // Unix path (simplified - matches paths starting with /)
    map.insert("path", vec![Regex::new(r"\B(/[\w./-]+)").unwrap()]);

    // OAuth token (Google-style)
    map.insert(
        "oauth",
        vec![Regex::new(r"\bya29\.[0-9A-Za-z_-]+\b").unwrap()],
    );

    // Function calls
    map.insert("function", vec![Regex::new(r"\b[\w\.]+\([^)]*\)").unwrap()]);

    // Hex color
    map.insert("hexcolor", vec![Regex::new(r"#[0-9A-Fa-f]{6}\b").unwrap()]);

    // Version string
    map.insert(
        "version",
        vec![Regex::new(r"\b[vV]\d+\.\d+(?:\.\d+)?(?:-[a-zA-Z0-9]+)?\b").unwrap()],
    );

    // Hex number
    map.insert("hexnum", vec![Regex::new(r"\b0x[0-9a-fA-F]+\b").unwrap()]);

    // Duration patterns (simplified without look-around)
    map.insert(
        "duration",
        vec![
            // Basic with units (1h, 30m, 5s, etc.) - use word boundaries
            Regex::new(r"\b\d+(?:\.\d+)?(?:us|ms|[smhd])\b").unwrap(),
            // Written out units
            Regex::new(r"\b\d+(?:\.\d+)?\s*(?:microsecond|millisecond|second|minute|hour|day|week|month|year)s?\b").unwrap(),
            // Combined (1h30m, 2h15m30s)
            Regex::new(r"\b(?:\d+h\d+m\d+s|\d+h\d+m|\d+h\d+s|\d+m\d+s)\b").unwrap(),
        ],
    );

    // Generic number (most aggressive - place last)
    // Requires at least one digit before decimal point to avoid matching ".123" in IPs/paths
    map.insert(
        "num",
        vec![Regex::new(r"[+-]?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?").unwrap()],
    );

    // Credit card number (13-19 digits, optionally separated by spaces or hyphens)
    // Validated with Luhn algorithm in the replacement function
    map.insert(
        "credit_card",
        vec![Regex::new(r"\b\d(?:[ -]?\d){12,18}\b").unwrap()],
    );

    // US Social Security Number (XXX-XX-XXXX format)
    map.insert("ssn", vec![Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap()]);

    // Phone numbers (various formats)
    map.insert(
        "phone",
        vec![
            // International format: +1-234-567-8900 or +1 234 567 8900
            Regex::new(r"\+\d{1,3}[-.\s]?\(?\d{1,4}\)?[-.\s]?\d{1,4}[-.\s]?\d{1,9}").unwrap(),
            // US format with parentheses: (123) 456-7890
            Regex::new(r"\(\d{3}\)[-.\s]?\d{3}[-.\s]?\d{4}").unwrap(),
            // US format without parentheses: 123-456-7890
            Regex::new(r"\b\d{3}[-.\s]\d{3}[-.\s]\d{4}\b").unwrap(),
        ],
    );

    map
});

/// Patterns that require additional validation beyond regex matching
type Validator = fn(&str) -> bool;
static VALIDATORS: Lazy<HashMap<&'static str, Validator>> = Lazy::new(|| {
    let mut map: HashMap<&'static str, Validator> = HashMap::new();
    map.insert("credit_card", is_valid_luhn as Validator);
    map
});

/// Default pattern set - balanced between specificity and utility
/// Note: PII patterns (credit_card, ssn, phone) are NOT in defaults for safety
const DEFAULT_PATTERNS: &[&str] = &[
    "ipv4_port",
    "ipv4",
    "ipv6",
    "email",
    "url",
    "fqdn",
    "uuid",
    "mac",
    "md5",
    "sha1",
    "sha256",
    "path",
    "oauth",
    "function",
    "hexcolor",
    "version",
];

/// Parse pattern specification (CSV string or array) into Vec of pattern names
fn parse_patterns(spec: Dynamic) -> Result<Vec<String>, String> {
    if spec.is_string() {
        // CSV string like "ipv4,email,url"
        let s = spec
            .into_string()
            .map_err(|_| "Failed to convert to string")?;
        Ok(s.split(',').map(|p| p.trim().to_string()).collect())
    } else if spec.is_array() {
        // Array like ["ipv4", "email", "url"]
        let arr = spec
            .into_array()
            .map_err(|_| "Failed to convert to array")?;
        arr.into_iter()
            .map(|v| {
                v.into_string()
                    .map_err(|_| "Array element is not a string".to_string())
            })
            .collect()
    } else {
        Err("Pattern spec must be a string or array".to_string())
    }
}

/// Core normalization logic with two-pass replacement
fn normalized_str_impl(text: &str, patterns: &[String]) -> String {
    let mut result = text.to_string();
    let mut replacements: Vec<(char, String)> = Vec::new();

    // First pass: replace matches with unique temporary markers
    // Use Unicode private use area (U+E000-U+F8FF) to avoid conflicts
    for (idx, pattern_name) in patterns.iter().enumerate() {
        let placeholder = format!("<{}>", pattern_name);
        let validator = VALIDATORS.get(pattern_name.as_str()).copied();

        // Regex-based patterns
        if let Some(regexes) = PATTERNS.get(pattern_name.as_str()) {
            for regex in regexes {
                if let Some(marker) = char::from_u32(0xE000 + idx as u32) {
                    replacements.push((marker, placeholder.clone()));

                    if let Some(validate) = validator {
                        // Pattern requires validation (e.g., credit_card with Luhn)
                        result = regex
                            .replace_all(&result, |caps: &Captures| {
                                let matched = &caps[0];
                                if validate(matched) {
                                    marker.to_string()
                                } else {
                                    matched.to_string()
                                }
                            })
                            .to_string();
                    } else {
                        // Simple regex replacement
                        result = regex.replace_all(&result, marker.to_string()).to_string();
                    }
                }
            }
        }
    }

    // Second pass: replace temporary markers with final placeholders
    for (marker, placeholder) in replacements {
        result = result.replace(marker, &placeholder);
    }

    result
}

/// Normalize a string with default patterns
fn normalized_str_default(text: &str) -> String {
    let patterns: Vec<String> = DEFAULT_PATTERNS.iter().map(|s| s.to_string()).collect();
    normalized_str_impl(text, &patterns)
}

/// Normalize a string with specified patterns
fn normalized_str_with_patterns(
    text: &str,
    spec: Dynamic,
) -> Result<String, Box<rhai::EvalAltResult>> {
    let patterns = parse_patterns(spec).map_err(|e| {
        Box::new(rhai::EvalAltResult::ErrorRuntime(
            e.into(),
            rhai::Position::NONE,
        ))
    })?;
    Ok(normalized_str_impl(text, &patterns))
}

/// Recursively normalize all string values in a map
fn normalized_map_impl(map: &mut Map, patterns: &[String]) {
    for (_key, value) in map.iter_mut() {
        if value.is_string() {
            if let Ok(s) = value.clone().into_string() {
                *value = Dynamic::from(normalized_str_impl(&s, patterns));
            }
        } else if value.is_map() {
            if let Some(mut nested_map) = value.clone().try_cast::<Map>() {
                normalized_map_impl(&mut nested_map, patterns);
                *value = Dynamic::from(nested_map);
            }
        } else if value.is_array() {
            if let Ok(mut arr) = value.clone().into_array() {
                for item in arr.iter_mut() {
                    if item.is_string() {
                        if let Ok(s) = item.clone().into_string() {
                            *item = Dynamic::from(normalized_str_impl(&s, patterns));
                        }
                    } else if item.is_map() {
                        if let Some(mut nested_map) = item.clone().try_cast::<Map>() {
                            normalized_map_impl(&mut nested_map, patterns);
                            *item = Dynamic::from(nested_map);
                        }
                    }
                }
                *value = Dynamic::from(arr);
            }
        }
    }
}

/// Normalize a map with default patterns
fn normalized_map_default(mut map: Map) -> Map {
    let patterns: Vec<String> = DEFAULT_PATTERNS.iter().map(|s| s.to_string()).collect();
    normalized_map_impl(&mut map, &patterns);
    map
}

/// Normalize a map with specified patterns
fn normalized_map_with_patterns(
    mut map: Map,
    spec: Dynamic,
) -> Result<Map, Box<rhai::EvalAltResult>> {
    let patterns = parse_patterns(spec).map_err(|e| {
        Box::new(rhai::EvalAltResult::ErrorRuntime(
            e.into(),
            rhai::Position::NONE,
        ))
    })?;
    normalized_map_impl(&mut map, &patterns);
    Ok(map)
}

pub fn register_functions(engine: &mut Engine) {
    // String normalization - default patterns
    engine.register_fn("normalized", normalized_str_default);

    // String normalization - with pattern spec (CSV or array)
    engine.register_fn("normalized", normalized_str_with_patterns);

    // Map normalization - default patterns
    engine.register_fn("normalized", normalized_map_default);

    // Map normalization - with pattern spec (CSV or array)
    engine.register_fn("normalized", normalized_map_with_patterns);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalized_ipv4() {
        let result = normalized_str_impl("Server at 192.168.1.100 failed", &["ipv4".to_string()]);
        assert_eq!(result, "Server at <ipv4> failed");
    }

    #[test]
    fn test_normalized_email() {
        let result =
            normalized_str_impl("Contact user@example.com for help", &["email".to_string()]);
        assert_eq!(result, "Contact <email> for help");
    }

    #[test]
    fn test_normalized_url() {
        let result = normalized_str_impl("Visit https://example.com/path", &["url".to_string()]);
        assert_eq!(result, "Visit <url>");
    }

    #[test]
    fn test_normalized_uuid() {
        let result = normalized_str_impl(
            "Request 550e8400-e29b-41d4-a716-446655440000 processed",
            &["uuid".to_string()],
        );
        assert_eq!(result, "Request <uuid> processed");
    }

    #[test]
    fn test_normalized_multiple_patterns() {
        let result = normalized_str_impl(
            "User user@example.com from 10.0.0.5 accessed https://api.example.com",
            &["ipv4".to_string(), "email".to_string(), "url".to_string()],
        );
        assert_eq!(result, "User <email> from <ipv4> accessed <url>");
    }

    #[test]
    fn test_normalized_default_patterns() {
        let result = normalized_str_default(
            "User user@example.com from 192.168.1.1 with UUID 550e8400-e29b-41d4-a716-446655440000",
        );
        assert!(result.contains("<email>"));
        assert!(result.contains("<ipv4>"));
        assert!(result.contains("<uuid>"));
    }

    #[test]
    fn test_parse_patterns_csv() {
        let spec = Dynamic::from("ipv4,email,url");
        let patterns = parse_patterns(spec).unwrap();
        assert_eq!(patterns, vec!["ipv4", "email", "url"]);
    }

    #[test]
    fn test_parse_patterns_array() {
        let arr = vec![Dynamic::from("ipv4"), Dynamic::from("email")];
        let spec = Dynamic::from(arr);
        let patterns = parse_patterns(spec).unwrap();
        assert_eq!(patterns, vec!["ipv4", "email"]);
    }

    #[test]
    fn test_normalized_map_basic() {
        let mut map = Map::new();
        map.insert("message".into(), Dynamic::from("IP: 192.168.1.1"));
        map.insert("email".into(), Dynamic::from("test@example.com"));

        let patterns = vec!["ipv4".to_string(), "email".to_string()];
        let mut result = map.clone();
        normalized_map_impl(&mut result, &patterns);

        assert_eq!(
            result
                .get("message")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "IP: <ipv4>"
        );
        assert_eq!(
            result.get("email").unwrap().clone().into_string().unwrap(),
            "<email>"
        );
    }

    #[test]
    fn test_two_pass_no_corruption() {
        // Ensure that placeholder text doesn't get partially replaced
        let result = normalized_str_impl(
            "email: user@example.com color: #FF0000",
            &["email".to_string(), "hexcolor".to_string()],
        );
        assert_eq!(result, "email: <email> color: <hexcolor>");
        // Verify <email> didn't get corrupted by hexcolor pattern
        assert!(!result.contains("<hexcol<email>"));
    }

    #[test]
    fn test_normalized_hash_values() {
        let md5 = "5d41402abc4b2a76b9719d911017c592";
        let result = normalized_str_impl(&format!("MD5: {}", md5), &["md5".to_string()]);
        assert_eq!(result, "MD5: <md5>");

        let sha256 = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let result = normalized_str_impl(&format!("SHA256: {}", sha256), &["sha256".to_string()]);
        assert_eq!(result, "SHA256: <sha256>");
    }

    #[test]
    fn test_normalized_version() {
        let result = normalized_str_impl("Version v1.2.3 released", &["version".to_string()]);
        assert_eq!(result, "Version <version> released");
    }

    #[test]
    fn test_normalized_mac_address() {
        let result = normalized_str_impl("MAC: 00:1A:2B:3C:4D:5E", &["mac".to_string()]);
        assert_eq!(result, "MAC: <mac>");

        let result = normalized_str_impl("MAC: 001A.2B3C.4D5E", &["mac".to_string()]);
        assert_eq!(result, "MAC: <mac>");
    }

    #[test]
    fn test_normalized_num_no_leading_dot() {
        // The integer part after leading dot will match, but not the dot itself
        let result = normalized_str_impl("value .113 found", &["num".to_string()]);
        assert_eq!(result, "value .<num> found");

        // Valid decimals with leading digit still work
        let result = normalized_str_impl("value 0.113 found", &["num".to_string()]);
        assert_eq!(result, "value <num> found");
    }

    #[test]
    fn test_normalized_num_with_ip_address() {
        // IP addresses with num pattern: matches decimal numbers like "203.0" and "113.1"
        // This is better than old behavior but still not ideal - use ipv4 pattern instead
        let result = normalized_str_impl("Server at 203.0.113.1 failed", &["num".to_string()]);
        assert_eq!(result, "Server at <num>.<num> failed");

        // Proper way: use ipv4 pattern for IP addresses
        let result = normalized_str_impl("Server at 203.0.113.1 failed", &["ipv4".to_string()]);
        assert_eq!(result, "Server at <ipv4> failed");
    }

    #[test]
    fn test_normalized_num_valid_numbers() {
        // Integers
        let result = normalized_str_impl("count: 42", &["num".to_string()]);
        assert_eq!(result, "count: <num>");

        // Decimals with leading digit
        let result = normalized_str_impl("pi: 3.14159", &["num".to_string()]);
        assert_eq!(result, "pi: <num>");

        // Negative numbers
        let result = normalized_str_impl("temp: -42.5", &["num".to_string()]);
        assert_eq!(result, "temp: <num>");

        // Scientific notation
        let result = normalized_str_impl("val: 1.23e-10", &["num".to_string()]);
        assert_eq!(result, "val: <num>");
    }

    // PII pattern tests

    #[test]
    fn test_luhn_validation() {
        // Valid test card numbers
        assert!(is_valid_luhn("4111111111111111")); // Visa test
        assert!(is_valid_luhn("5500000000000004")); // Mastercard test
        assert!(is_valid_luhn("378282246310005")); // Amex test
        assert!(is_valid_luhn("4111-1111-1111-1111")); // With dashes
        assert!(is_valid_luhn("4111 1111 1111 1111")); // With spaces

        // Invalid numbers
        assert!(!is_valid_luhn("4111111111111112")); // Bad checksum
        assert!(!is_valid_luhn("1234567890")); // Too short
        assert!(!is_valid_luhn("0000000000000000")); // Not enough distinct digits
    }

    #[test]
    fn test_normalized_credit_card() {
        // Valid credit card should be replaced
        let result = normalized_str_impl(
            "Card: 4111111111111111 charged",
            &["credit_card".to_string()],
        );
        assert_eq!(result, "Card: <credit_card> charged");

        // With dashes
        let result = normalized_str_impl(
            "Card: 4111-1111-1111-1111 charged",
            &["credit_card".to_string()],
        );
        assert_eq!(result, "Card: <credit_card> charged");

        // Invalid card number should NOT be replaced
        let result =
            normalized_str_impl("Not a card: 4111111111111112", &["credit_card".to_string()]);
        assert_eq!(result, "Not a card: 4111111111111112");
    }

    #[test]
    fn test_normalized_ssn() {
        let result = normalized_str_impl("SSN: 123-45-6789", &["ssn".to_string()]);
        assert_eq!(result, "SSN: <ssn>");

        // Should not match without dashes
        let result = normalized_str_impl("Not SSN: 123456789", &["ssn".to_string()]);
        assert_eq!(result, "Not SSN: 123456789");
    }

    #[test]
    fn test_normalized_phone() {
        // US format with parentheses
        let result = normalized_str_impl("Call (555) 123-4567", &["phone".to_string()]);
        assert_eq!(result, "Call <phone>");

        // US format with dashes
        let result = normalized_str_impl("Call 555-123-4567", &["phone".to_string()]);
        assert_eq!(result, "Call <phone>");

        // International format
        let result = normalized_str_impl("Call +1-555-123-4567", &["phone".to_string()]);
        assert_eq!(result, "Call <phone>");
    }

    #[test]
    fn test_normalized_pii_combined() {
        let result = normalized_str_impl(
            "User SSN 123-45-6789, card 4111111111111111, phone (555) 123-4567",
            &[
                "ssn".to_string(),
                "credit_card".to_string(),
                "phone".to_string(),
            ],
        );
        assert_eq!(result, "User SSN <ssn>, card <credit_card>, phone <phone>");
    }
}
