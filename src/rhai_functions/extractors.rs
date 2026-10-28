//! Pattern extraction functions for Rhai scripts.
//!
//! Provides functions for extracting patterns (IPs, URLs, emails, JSON) from text.

use crate::event::json_to_dynamic;
use regex::Regex;
use rhai::{Array, Dynamic, Engine};
use std::net::Ipv6Addr;
use std::str::FromStr;
use std::sync::LazyLock;

// Regex patterns (IPv4 validates 0-255 range for each octet)
const IPV4_PATTERN: &str = r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b";
// IPv6 candidates (validated by `Ipv6Addr` afterwards). The first branch keeps an
// embedded IPv4 tail (`::ffff:10.0.0.1`) in the same match.
const IPV6_CANDIDATE_PATTERN: &str =
    r"[0-9A-Fa-f:]*:[0-9]{1,3}(?:\.[0-9]{1,3}){3}|[0-9A-Fa-f:]*:[0-9A-Fa-f:]*";
const URL_PATTERN: &str = r##"https?://[^\s<>"]+[^\s<>".,;!?]"##;
const URL_DOMAIN_PATTERN: &str = r##"https?://([^/\s<>"]+)"##;
const EMAIL_PATTERN: &str = r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b";
const EMAIL_DOMAIN_PATTERN: &str = r##"[a-zA-Z0-9._%+-]+@([a-zA-Z0-9.-]+\.[a-zA-Z]{2,})"##;

// Compiled regex instances
static IPV4_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(IPV4_PATTERN).expect("failed to compile IPv4 regex"));
static IPV6_CANDIDATE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(IPV6_CANDIDATE_PATTERN).expect("failed to compile IPv6 regex"));
static URL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(URL_PATTERN).expect("failed to compile URL regex"));
static URL_DOMAIN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(URL_DOMAIN_PATTERN).expect("failed to compile URL domain regex"));
static EMAIL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(EMAIL_PATTERN).expect("failed to compile email regex"));
static EMAIL_DOMAIN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(EMAIL_DOMAIN_PATTERN).expect("failed to compile email domain regex")
});

// ============================================================================
// IP Extraction
// ============================================================================

/// Find IPv4 and IPv6 addresses in text order
fn find_ips(text: &str) -> Vec<&str> {
    let v4 = IPV4_REGEX.find_iter(text).map(|m| (m.start(), m.end()));

    // Cheap prefilter: an IPv6 address has a `::` or at least six colons
    let maybe_v6 = text.contains("::") || text.bytes().filter(|&b| b == b':').count() >= 6;
    if !maybe_v6 {
        return v4.map(|(s, e)| &text[s..e]).collect();
    }

    let v6: Vec<(usize, usize)> = IPV6_CANDIDATE_REGEX
        .find_iter(text)
        .filter_map(|m| ipv6_span(text, m.start(), m.end()))
        .collect();

    // Drop IPv4 matches inside an IPv6 address (`::ffff:10.0.0.1`)
    let mut spans: Vec<(usize, usize)> = v4
        .filter(|&(s, e)| !v6.iter().any(|&(vs, ve)| s >= vs && e <= ve))
        .chain(v6.iter().copied())
        .collect();
    spans.sort_unstable();
    spans.into_iter().map(|(s, e)| &text[s..e]).collect()
}

/// Validate an IPv6 candidate, returning the span of the address
fn ipv6_span(text: &str, start: usize, mut end: usize) -> Option<(usize, usize)> {
    // Must stand alone: `Pipeline::carry` or `T10:00:00` are not addresses
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    if text[..start]
        .chars()
        .next_back()
        .is_some_and(|c| is_word(c) || c == '.')
        || text[end..].chars().next().is_some_and(is_word)
    {
        return None;
    }

    // A colon that ends a sentence-like phrase: "from 2001:db8::1: refused"
    let candidate = &text[start..end];
    if candidate.ends_with(':') && !candidate.ends_with("::") {
        end -= 1;
    }
    let candidate = &text[start..end];

    if candidate == "::" || Ipv6Addr::from_str(candidate).is_err() {
        return None;
    }
    // Eight two-digit groups are a hardware ID (WWN, EUI-64), not an address
    if !candidate.contains("::") && candidate.split(':').all(|g| g.len() == 2) {
        return None;
    }
    Some((start, end))
}

/// Extract the first IP address from text
fn extract_ip_first(text: &str) -> String {
    find_ips(text)
        .first()
        .map(|ip| ip.to_string())
        .unwrap_or_default()
}

/// Extract the nth IP address from text (1-indexed, negative for from end)
fn extract_ip_nth(text: &str, nth: i64) -> String {
    if nth == 0 {
        return String::new();
    }

    let matches = find_ips(text);

    if matches.is_empty() {
        return String::new();
    }

    // Handle negative indexing (from the end)
    let idx = if nth < 0 {
        let abs_nth = (-nth) as usize;
        if abs_nth > matches.len() {
            return String::new();
        }
        matches.len() - abs_nth
    } else {
        let nth_usize = nth as usize;
        if nth_usize < 1 || nth_usize > matches.len() {
            return String::new();
        }
        nth_usize - 1 // Convert to 0-indexed
    };

    matches[idx].to_string()
}

/// Extract all IP addresses from text
fn extract_ips_impl(text: &str) -> Array {
    find_ips(text)
        .into_iter()
        .map(|ip| Dynamic::from(ip.to_string()))
        .collect()
}

// ============================================================================
// URL Extraction
// ============================================================================

/// Extract the first URL from text
fn extract_url_first(text: &str) -> String {
    URL_REGEX
        .find(text)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Extract the nth URL from text (1-indexed, negative for from end)
fn extract_url_nth(text: &str, nth: i64) -> String {
    if nth == 0 {
        return String::new();
    }

    let matches: Vec<_> = URL_REGEX.find_iter(text).collect();

    if matches.is_empty() {
        return String::new();
    }

    // Handle negative indexing (from the end)
    let idx = if nth < 0 {
        let abs_nth = (-nth) as usize;
        if abs_nth > matches.len() {
            return String::new();
        }
        matches.len() - abs_nth
    } else {
        let nth_usize = nth as usize;
        if nth_usize < 1 || nth_usize > matches.len() {
            return String::new();
        }
        nth_usize - 1 // Convert to 0-indexed
    };

    matches[idx].as_str().to_string()
}

/// Extract all URLs from text
fn extract_urls_impl(text: &str) -> Array {
    URL_REGEX
        .find_iter(text)
        .map(|m| Dynamic::from(m.as_str().to_string()))
        .collect()
}

// ============================================================================
// Email Extraction
// ============================================================================

/// Extract the first email address from text
fn extract_email_first(text: &str) -> String {
    EMAIL_REGEX
        .find(text)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Extract the nth email address from text (1-indexed, negative for from end)
fn extract_email_nth(text: &str, nth: i64) -> String {
    if nth == 0 {
        return String::new();
    }

    let matches: Vec<_> = EMAIL_REGEX.find_iter(text).collect();

    if matches.is_empty() {
        return String::new();
    }

    // Handle negative indexing (from the end)
    let idx = if nth < 0 {
        let abs_nth = (-nth) as usize;
        if abs_nth > matches.len() {
            return String::new();
        }
        matches.len() - abs_nth
    } else {
        let nth_usize = nth as usize;
        if nth_usize < 1 || nth_usize > matches.len() {
            return String::new();
        }
        nth_usize - 1 // Convert to 0-indexed
    };

    matches[idx].as_str().to_string()
}

/// Extract all email addresses from text
fn extract_emails_impl(text: &str) -> Array {
    EMAIL_REGEX
        .find_iter(text)
        .map(|m| Dynamic::from(m.as_str().to_string()))
        .collect()
}

// ============================================================================
// Domain Extraction
// ============================================================================

/// Extract domain from URL or email in text
fn extract_domain_impl(text: &str) -> String {
    // Try URL first, then email domain
    if let Some(caps) = URL_DOMAIN_REGEX.captures(text) {
        if let Some(domain) = caps.get(1) {
            return domain.as_str().to_string();
        }
    }

    if let Some(caps) = EMAIL_DOMAIN_REGEX.captures(text) {
        if let Some(domain) = caps.get(1) {
            return domain.as_str().to_string();
        }
    }

    String::new()
}

// ============================================================================
// JSON Extraction
// ============================================================================

/// Extract JSON objects or arrays from text
///
/// Searches through the text for valid JSON objects `{...}` or arrays `[...]`
/// and returns the nth occurrence (0-indexed).
///
/// # Arguments
/// * `text` - The input string to search for JSON
/// * `nth` - Which occurrence to extract (0 = first, 1 = second, etc.)
///
/// # Returns
/// A Rhai Dynamic containing the parsed JSON (Map for objects, Array for arrays),
/// or an empty string if not found or parsing fails
fn extract_json_impl(text: &str, nth: i64) -> Dynamic {
    // nth follows the same convention as the other extractors:
    // 1 = first, 2 = second, ..., -1 = last, -2 = second to last. 0 is invalid.
    if text.is_empty() || nth == 0 {
        return Dynamic::from(String::new());
    }

    let matches = collect_json_values(text);
    if matches.is_empty() {
        return Dynamic::from(String::new());
    }

    // Handle negative indexing (from the end)
    let idx = if nth < 0 {
        let abs_nth = (-nth) as usize;
        if abs_nth > matches.len() {
            return Dynamic::from(String::new());
        }
        matches.len() - abs_nth
    } else {
        let nth_usize = nth as usize;
        if nth_usize > matches.len() {
            return Dynamic::from(String::new());
        }
        nth_usize - 1 // Convert to 0-indexed
    };

    json_to_dynamic(&matches[idx])
}

/// Scan text for all top-level JSON objects/arrays, in order of appearance.
///
/// Handles nested structures and strings (so braces inside string values do
/// not confuse the scanner). Invalid or non-object/array candidates are skipped.
fn collect_json_values(text: &str) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();

    // Iterate through each character looking for JSON start markers
    let mut i = 0;
    while i < len {
        let ch = bytes[i] as char;

        // Check if this is a potential JSON start
        if ch != '{' && ch != '[' {
            i += 1;
            continue;
        }

        let start_char = ch;
        let end_char = if ch == '{' { '}' } else { ']' };

        // Try to find matching closing bracket/brace
        // We need to handle nested structures
        let mut depth = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut end_pos = None;

        #[allow(clippy::needless_range_loop)]
        for j in i..len {
            let current = bytes[j] as char;

            if escape_next {
                escape_next = false;
                continue;
            }

            if current == '\\' && in_string {
                escape_next = true;
                continue;
            }

            if current == '"' {
                in_string = !in_string;
                continue;
            }

            if in_string {
                continue;
            }

            if current == start_char {
                depth += 1;
            } else if current == end_char {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(j + 1);
                    break;
                }
            }
        }

        // If we found a complete structure, try to parse it
        if let Some(end) = end_pos {
            let candidate = &text[i..end];

            // Try to parse as JSON
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(candidate) {
                // Check if it's an object or array
                if json_value.is_object() || json_value.is_array() {
                    results.push(json_value);
                }
            }

            // Move past this structure
            i = end;
        } else {
            // No matching closing bracket found, move to next character
            i += 1;
        }
    }

    results
}

/// Extract all JSON objects or arrays from text as strings
///
/// Searches through the text for all valid JSON objects `{...}` or arrays `[...]`
/// and returns them as an array of strings.
///
/// # Arguments
/// * `text` - The input string to search for JSON
///
/// # Returns
/// A Rhai Array containing all found JSON strings (raw JSON text)
fn extract_jsons_impl(text: &str) -> Array {
    let mut results = Array::new();

    if text.is_empty() {
        return results;
    }

    let bytes = text.as_bytes();
    let len = bytes.len();

    // Iterate through each character looking for JSON start markers
    let mut i = 0;
    while i < len {
        let ch = bytes[i] as char;

        // Check if this is a potential JSON start
        if ch != '{' && ch != '[' {
            i += 1;
            continue;
        }

        let start_char = ch;
        let end_char = if ch == '{' { '}' } else { ']' };

        // Try to find matching closing bracket/brace
        // We need to handle nested structures
        let mut depth = 0;
        let mut in_string = false;
        let mut escape_next = false;
        let mut end_pos = None;

        #[allow(clippy::needless_range_loop)]
        for j in i..len {
            let current = bytes[j] as char;

            if escape_next {
                escape_next = false;
                continue;
            }

            if current == '\\' && in_string {
                escape_next = true;
                continue;
            }

            if current == '"' {
                in_string = !in_string;
                continue;
            }

            if in_string {
                continue;
            }

            if current == start_char {
                depth += 1;
            } else if current == end_char {
                depth -= 1;
                if depth == 0 {
                    end_pos = Some(j + 1);
                    break;
                }
            }
        }

        // If we found a complete structure, try to parse it
        if let Some(end) = end_pos {
            let candidate = &text[i..end];

            // Try to parse as JSON to validate
            if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(candidate) {
                // Check if it's an object or array
                if json_value.is_object() || json_value.is_array() {
                    // Add the raw JSON string to results
                    results.push(Dynamic::from(candidate.to_string()));
                }
            }

            // Move past this structure
            i = end;
        } else {
            // No matching closing bracket found, move to next character
            i += 1;
        }
    }

    results
}

// ============================================================================
// Registration
// ============================================================================

/// Register all extraction functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // IP extraction
    engine.register_fn("extract_ip", extract_ip_first);
    engine.register_fn("extract_ip", extract_ip_nth);
    engine.register_fn("extract_ips", extract_ips_impl);

    // URL extraction
    engine.register_fn("extract_url", extract_url_first);
    engine.register_fn("extract_url", extract_url_nth);
    engine.register_fn("extract_urls", extract_urls_impl);

    // Email extraction
    engine.register_fn("extract_email", extract_email_first);
    engine.register_fn("extract_email", extract_email_nth);
    engine.register_fn("extract_emails", extract_emails_impl);

    // Domain extraction
    engine.register_fn("extract_domain", extract_domain_impl);

    // JSON extraction
    engine.register_fn("extract_json", |text: &str| -> Dynamic {
        extract_json_impl(text, 1)
    });
    engine.register_fn("extract_json", |text: &str, nth: i64| -> Dynamic {
        extract_json_impl(text, nth)
    });
    engine.register_fn("extract_jsons", extract_jsons_impl);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Scope;

    // ========================================================================
    // IP Extraction Tests
    // ========================================================================

    #[test]
    fn test_extract_ip_basic() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Connection from 192.168.1.100 to 10.0.0.1");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_ip(text)"#)
            .unwrap();
        assert_eq!(result, "192.168.1.100");
    }

    #[test]
    fn test_extract_ip_nth() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "IPs: 192.168.1.1, 10.0.0.1, 172.16.0.1");

        // First IP (1-indexed)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_ip(text, 1)"#)
            .unwrap();
        assert_eq!(result, "192.168.1.1");

        // Second IP
        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_ip(text, 2)"#)
            .unwrap();
        assert_eq!(result, "10.0.0.1");

        // Last IP (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_ip(text, -1)"#)
            .unwrap();
        assert_eq!(result, "172.16.0.1");
    }

    #[test]
    fn test_extract_ips() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "IPs: 192.168.1.1, 10.0.0.1, 172.16.0.1");

        let result: Array = engine
            .eval_with_scope(&mut scope, r#"extract_ips(text)"#)
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].clone().into_string().unwrap(), "192.168.1.1");
        assert_eq!(result[1].clone().into_string().unwrap(), "10.0.0.1");
        assert_eq!(result[2].clone().into_string().unwrap(), "172.16.0.1");
    }

    fn ips(text: &str) -> Vec<String> {
        find_ips(text).into_iter().map(String::from).collect()
    }

    #[test]
    fn test_extract_ip_ipv6() {
        assert_eq!(
            extract_ip_first("Failed password for root from 2001:db8::7 port 22 ssh2"),
            "2001:db8::7"
        );
        assert_eq!(ips("from ::1 to fe80::1%eth0"), ["::1", "fe80::1"]);
        assert_eq!(
            ips("full 2001:0db8:0000:0000:0000:0000:0000:0001 here"),
            ["2001:0db8:0000:0000:0000:0000:0000:0001"]
        );
        assert_eq!(
            ips("[2001:db8::1]:443 and 2001:db8::2."),
            ["2001:db8::1", "2001:db8::2"]
        );
        assert_eq!(ips("connect to 2001:db8::1: refused"), ["2001:db8::1"]);
    }

    #[test]
    fn test_extract_ip_mixed_families_in_text_order() {
        let text = "XFF: 2001:db8::5, 10.0.0.1";
        assert_eq!(extract_ip_first(text), "2001:db8::5");
        assert_eq!(extract_ip_nth(text, -1), "10.0.0.1");
        assert_eq!(
            ips("10.0.0.1 then 2001:db8::5 then 10.0.0.2"),
            ["10.0.0.1", "2001:db8::5", "10.0.0.2"]
        );
    }

    #[test]
    fn test_extract_ip_ipv4_mapped() {
        assert_eq!(
            ips("client ::ffff:10.0.0.1 via 2001:db8::1"),
            ["::ffff:10.0.0.1", "2001:db8::1"]
        );
    }

    #[test]
    fn test_extract_ip_ipv6_false_positives() {
        assert!(ips("at 12:34:56.789 ts=2024-01-01T10:00:00Z").is_empty());
        assert!(ips("mac aa:bb:cc:dd:ee:ff wwn 50:06:01:60:3b:20:19:a8").is_empty());
        assert!(ips(r"Illuminate\Pipeline\Pipeline::carry() Foo::bad std::io").is_empty());
        assert!(ips("Bind-address: '::' port: 33060").is_empty());
        assert_eq!(ips("10.0.0.1:8080 and ::1"), ["10.0.0.1", "::1"]);
    }

    // ========================================================================
    // URL Extraction Tests
    // ========================================================================

    #[test]
    fn test_extract_url_basic() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Visit https://example.com/page for more info");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_url(text)"#)
            .unwrap();
        assert_eq!(result, "https://example.com/page");
    }

    #[test]
    fn test_extract_url_nth() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Links: https://a.com https://b.com https://c.com");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_url(text, 2)"#)
            .unwrap();
        assert_eq!(result, "https://b.com");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_url(text, -1)"#)
            .unwrap();
        assert_eq!(result, "https://c.com");
    }

    #[test]
    fn test_extract_urls() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Links: https://a.com https://b.com");

        let result: Array = engine
            .eval_with_scope(&mut scope, r#"extract_urls(text)"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].clone().into_string().unwrap(), "https://a.com");
        assert_eq!(result[1].clone().into_string().unwrap(), "https://b.com");
    }

    // ========================================================================
    // Email Extraction Tests
    // ========================================================================

    #[test]
    fn test_extract_email_basic() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Contact user@example.com for support");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_email(text)"#)
            .unwrap();
        assert_eq!(result, "user@example.com");
    }

    #[test]
    fn test_extract_email_nth() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Emails: a@x.com, b@y.com, c@z.com");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_email(text, 2)"#)
            .unwrap();
        assert_eq!(result, "b@y.com");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_email(text, -1)"#)
            .unwrap();
        assert_eq!(result, "c@z.com");
    }

    #[test]
    fn test_extract_emails() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Emails: a@x.com, b@y.com");

        let result: Array = engine
            .eval_with_scope(&mut scope, r#"extract_emails(text)"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].clone().into_string().unwrap(), "a@x.com");
        assert_eq!(result[1].clone().into_string().unwrap(), "b@y.com");
    }

    // ========================================================================
    // Domain Extraction Tests
    // ========================================================================

    #[test]
    fn test_extract_domain_from_url() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Visit https://example.com/page");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_domain(text)"#)
            .unwrap();
        assert_eq!(result, "example.com");
    }

    #[test]
    fn test_extract_domain_from_email() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Contact user@example.org");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_domain(text)"#)
            .unwrap();
        assert_eq!(result, "example.org");
    }

    // ========================================================================
    // JSON Extraction Tests
    // ========================================================================

    #[test]
    fn test_extract_json_basic() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            r#"Log message: {"level": "error", "msg": "failed"} end"#,
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"extract_json(text)"#)
            .unwrap();

        assert_eq!(
            result.get("level").unwrap().clone().into_string().unwrap(),
            "error"
        );
        assert_eq!(
            result.get("msg").unwrap().clone().into_string().unwrap(),
            "failed"
        );
    }

    #[test]
    fn test_extract_json_nth() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            r#"First: {"a": 1} Second: {"b": 2} Third: {"c": 3}"#,
        );

        // 1-indexed, matching extract_ip/url/email: 1 = first
        let first: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"extract_json(text, 1)"#)
            .unwrap();
        assert_eq!(first.get("a").unwrap().clone().as_int().unwrap(), 1);

        // 2 = second
        let second: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"extract_json(text, 2)"#)
            .unwrap();
        assert_eq!(second.get("b").unwrap().clone().as_int().unwrap(), 2);

        // -1 = last
        let last: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"extract_json(text, -1)"#)
            .unwrap();
        assert_eq!(last.get("c").unwrap().clone().as_int().unwrap(), 3);

        // 0 is invalid -> empty string
        let zero: String = engine
            .eval_with_scope(&mut scope, r#"extract_json(text, 0)"#)
            .unwrap();
        assert!(zero.is_empty());

        // Out-of-range -> empty string
        let oor: String = engine
            .eval_with_scope(&mut scope, r#"extract_json(text, 4)"#)
            .unwrap();
        assert!(oor.is_empty());
    }

    #[test]
    fn test_extract_jsons() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", r#"{"a": 1} text {"b": 2} more [1, 2, 3]"#);

        let result: Array = engine
            .eval_with_scope(&mut scope, r#"extract_jsons(text)"#)
            .unwrap();

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_extract_json_not_found() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "no json here");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_json(text)"#)
            .unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_ip_zero_index() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "IP: 192.168.1.1");

        // Zero index should return empty (invalid)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"extract_ip(text, 0)"#)
            .unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_extract_no_match() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "no patterns here");

        let ip: String = engine
            .eval_with_scope(&mut scope, r#"extract_ip(text)"#)
            .unwrap();
        assert!(ip.is_empty());

        let url: String = engine
            .eval_with_scope(&mut scope, r#"extract_url(text)"#)
            .unwrap();
        assert!(url.is_empty());

        let email: String = engine
            .eval_with_scope(&mut scope, r#"extract_email(text)"#)
            .unwrap();
        assert!(email.is_empty());
    }
}
