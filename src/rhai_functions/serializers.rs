//! Serializer functions for converting Rhai Maps to various log formats.
//!
//! This module provides to_* functions that are the inverse of parse_* functions:
//! - `to_logfmt()` - Convert Map to logfmt format
//! - `to_kv()` - Convert Map to key-value format with configurable separators
//! - `to_syslog()` - Convert Map to RFC3164 syslog format
//! - `to_cef()` - Convert Map to Common Event Format (CEF)
//! - `to_combined()` - Convert Map to Apache/NGINX combined log format

use crate::event::{flatten_dynamic, FlattenStyle};
use rhai::Engine;

/// Register all serializer functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // to_logfmt() - Convert Map to logfmt format string
    engine.register_fn("to_logfmt", |map: rhai::Map| -> String {
        to_logfmt_impl(map)
    });

    // to_kv() - Multiple variants for flexible key-value formatting
    engine.register_fn("to_kv", |map: rhai::Map| -> String {
        to_kv_impl(map, None, "=")
    });

    engine.register_fn("to_kv", |map: rhai::Map, sep: &str| -> String {
        to_kv_impl(map, Some(sep), "=")
    });

    engine.register_fn(
        "to_kv",
        |map: rhai::Map, sep: &str, kv_sep: &str| -> String { to_kv_impl(map, Some(sep), kv_sep) },
    );

    // Allow unit type for null separator
    engine.register_fn(
        "to_kv",
        |map: rhai::Map, _sep: (), kv_sep: &str| -> String { to_kv_impl(map, None, kv_sep) },
    );

    // to_syslog() - Convert Map to syslog format string
    engine.register_fn("to_syslog", |map: rhai::Map| -> String {
        to_syslog_impl(map)
    });

    // to_cef() - Convert Map to CEF format string
    engine.register_fn("to_cef", |map: rhai::Map| -> String { to_cef_impl(map) });

    // to_combined() - Convert Map to combined log format string
    engine.register_fn("to_combined", |map: rhai::Map| -> String {
        to_combined_impl(map)
    });
}

// ============================================================================
// Implementation Functions
// ============================================================================

/// Convert a Rhai Map to logfmt format string
fn to_logfmt_impl(map: rhai::Map) -> String {
    let mut output = String::new();
    let mut first = true;

    for (key, value) in map {
        if !first {
            output.push(' ');
        }
        first = false;

        // Sanitize key for logfmt compliance
        let sanitized_key = sanitize_logfmt_key(&key);
        output.push_str(&sanitized_key);
        output.push('=');

        // Format value based on type
        let is_string = value.is_string();

        if value.clone().try_cast::<rhai::Map>().is_some()
            || value.clone().try_cast::<rhai::Array>().is_some()
        {
            // Handle nested structures by flattening
            let flattened = flatten_dynamic(&value, FlattenStyle::Underscore, 0);

            let formatted_value = if flattened.len() == 1 {
                flattened.values().next().unwrap().to_string()
            } else if flattened.is_empty() {
                String::new()
            } else {
                // Format as "key1=val1,key2=val2" for nested structures
                flattened
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join(",")
            };

            if is_string || needs_logfmt_quoting(&formatted_value) {
                format_quoted_logfmt_value(&formatted_value, &mut output);
            } else {
                output.push_str(&formatted_value);
            }
        } else {
            // Handle scalar values
            let string_val = value.to_string();
            if is_string {
                format_quoted_logfmt_value(&string_val, &mut output);
            } else {
                output.push_str(&string_val);
            }
        }
    }

    output
}

/// Convert a Rhai Map to key-value format with flexible separators
fn to_kv_impl(map: rhai::Map, sep: Option<&str>, kv_sep: &str) -> String {
    let mut output = String::new();
    let mut first = true;

    // Use whitespace as default separator if none specified
    let field_sep = sep.unwrap_or(" ");

    for (key, value) in map {
        if !first {
            output.push_str(field_sep);
        }
        first = false;

        // Key=value format
        let value_str = value.to_string();
        output.push_str(&key);
        output.push_str(kv_sep);

        // If using space as field separator and value contains spaces, quote it
        if field_sep == " " && value_str.contains(' ') {
            output.push('"');
            output.push_str(&value_str.replace('"', "\\\""));
            output.push('"');
        } else {
            output.push_str(&value_str);
        }
    }

    output
}

/// Convert a Rhai Map to RFC3164 syslog format
fn to_syslog_impl(map: rhai::Map) -> String {
    use chrono::Utc;

    // Standard syslog fields with defaults
    let priority = map
        .get("priority")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "13".to_string()); // user.notice

    let timestamp = map
        .get("timestamp")
        .map(|v| v.to_string())
        .unwrap_or_else(|| Utc::now().format("%b %d %H:%M:%S").to_string());

    let hostname = map
        .get("hostname")
        .or_else(|| map.get("host"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "localhost".to_string());

    let tag = map
        .get("tag")
        .or_else(|| map.get("program"))
        .or_else(|| map.get("ident"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "kelora".to_string());

    let message = map
        .get("message")
        .or_else(|| map.get("msg"))
        .or_else(|| map.get("content"))
        .map(|v| v.to_string())
        .unwrap_or_default();

    // RFC3164 format: <priority>timestamp hostname tag: message
    format!(
        "<{}>{} {} {}: {}",
        priority, timestamp, hostname, tag, message
    )
}

/// Convert a Rhai Map to Common Event Format (CEF)
fn to_cef_impl(map: rhai::Map) -> String {
    // CEF Header fields
    let device_vendor = map
        .get("deviceVendor")
        .or_else(|| map.get("device_vendor"))
        .map(|v| escape_cef_value(&v.to_string()))
        .unwrap_or_else(|| "Kelora".to_string());

    let device_product = map
        .get("deviceProduct")
        .or_else(|| map.get("device_product"))
        .map(|v| escape_cef_value(&v.to_string()))
        .unwrap_or_else(|| "LogAnalyzer".to_string());

    let device_version = map
        .get("deviceVersion")
        .or_else(|| map.get("device_version"))
        .map(|v| escape_cef_value(&v.to_string()))
        .unwrap_or_else(|| "1.0".to_string());

    let signature_id = map
        .get("signatureId")
        .or_else(|| map.get("signature_id"))
        .or_else(|| map.get("event_id"))
        .map(|v| escape_cef_value(&v.to_string()))
        .unwrap_or_else(|| "1".to_string());

    let name = map
        .get("name")
        .or_else(|| map.get("event_name"))
        .or_else(|| map.get("message"))
        .map(|v| escape_cef_value(&v.to_string()))
        .unwrap_or_else(|| "Event".to_string());

    let severity = map
        .get("severity")
        .or_else(|| map.get("level"))
        .map(|v| escape_cef_value(&v.to_string()))
        .unwrap_or_else(|| "5".to_string());

    // Start with CEF header
    let mut output = format!(
        "CEF:0|{}|{}|{}|{}|{}|{}|",
        device_vendor, device_product, device_version, signature_id, name, severity
    );

    // Add extension fields
    let mut extensions = Vec::new();
    for (key, value) in map {
        // Skip header fields we already processed
        if matches!(
            key.as_str(),
            "deviceVendor"
                | "device_vendor"
                | "deviceProduct"
                | "device_product"
                | "deviceVersion"
                | "device_version"
                | "signatureId"
                | "signature_id"
                | "event_id"
                | "name"
                | "event_name"
                | "message"
                | "severity"
                | "level"
        ) {
            continue;
        }

        extensions.push(format!(
            "{}={}",
            key,
            escape_cef_extension_value(&value.to_string())
        ));
    }

    if !extensions.is_empty() {
        output.push_str(&extensions.join(" "));
    }

    output
}

/// Convert a Rhai Map to Apache/NGINX combined log format
fn to_combined_impl(map: rhai::Map) -> String {
    use chrono::Utc;

    // Standard combined log format fields
    let ip = map
        .get("ip")
        .or_else(|| map.get("remote_addr"))
        .or_else(|| map.get("client_ip"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());

    let identity = map
        .get("identity")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());

    let user = map
        .get("user")
        .or_else(|| map.get("remote_user"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());

    let timestamp = map
        .get("timestamp")
        .map(|v| v.to_string())
        .unwrap_or_else(|| format!("[{}]", Utc::now().format("%d/%b/%Y:%H:%M:%S %z")));

    // Build request line from components or use provided request
    let request = if let Some(req) = map.get("request") {
        format!("\"{}\"", req.to_string().replace('"', "\\\""))
    } else {
        let method = map
            .get("method")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "GET".to_string());
        let path = map
            .get("path")
            .or_else(|| map.get("uri"))
            .map(|v| v.to_string())
            .unwrap_or_else(|| "/".to_string());
        let protocol = map
            .get("protocol")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "HTTP/1.1".to_string());
        format!("\"{} {} {}\"", method, path, protocol)
    };

    let status = map
        .get("status")
        .or_else(|| map.get("response_status"))
        .or_else(|| map.get("status_code"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "200".to_string());

    let bytes = map
        .get("bytes")
        .or_else(|| map.get("response_size"))
        .or_else(|| map.get("body_bytes_sent"))
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());

    let referer = map
        .get("referer")
        .or_else(|| map.get("http_referer"))
        .map(|v| format!("\"{}\"", v.to_string().replace('"', "\\\"")))
        .unwrap_or_else(|| "\"-\"".to_string());

    let user_agent = map
        .get("user_agent")
        .or_else(|| map.get("http_user_agent"))
        .map(|v| format!("\"{}\"", v.to_string().replace('"', "\\\"")))
        .unwrap_or_else(|| "\"-\"".to_string());

    // Basic combined format
    let mut output = format!(
        "{} {} {} {} {} {} {} {} {}",
        ip, identity, user, timestamp, request, status, bytes, referer, user_agent
    );

    // Add request_time if present (NGINX style)
    if let Some(request_time) = map.get("request_time") {
        output.push_str(&format!(" \"{}\"", request_time));
    }

    output
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Sanitize a field key to ensure logfmt compliance
fn sanitize_logfmt_key(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            c => c,
        })
        .collect()
}

/// Check if a string value needs to be quoted per logfmt rules
fn needs_logfmt_quoting(value: &str) -> bool {
    value.is_empty()
        || value.contains(' ')
        || value.contains('\t')
        || value.contains('\n')
        || value.contains('\r')
        || value.contains('\'')
        || value.contains('"')
        || value.contains('=')
}

/// Escape logfmt string by escaping quotes, backslashes, newlines, tabs, and carriage returns
fn escape_logfmt_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 10);

    for ch in input.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            c => output.push(c),
        }
    }

    output
}

/// Format a quoted logfmt value into a buffer
fn format_quoted_logfmt_value(value: &str, output: &mut String) {
    if needs_logfmt_quoting(value) {
        output.push('"');
        output.push_str(&escape_logfmt_string(value));
        output.push('"');
    } else {
        output.push_str(value);
    }
}

/// Escape CEF header field values (pipe characters)
fn escape_cef_value(value: &str) -> String {
    // Must escape backslashes first, then pipes, to avoid double-escaping
    value.replace('\\', "\\\\").replace('|', "\\|")
}

/// Escape CEF extension field values (equals and backslashes)
fn escape_cef_extension_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('=', "\\=")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Dynamic;

    #[test]
    fn test_to_logfmt_basic() {
        let mut map = rhai::Map::new();
        map.insert("level".into(), Dynamic::from("info"));
        map.insert("msg".into(), Dynamic::from("test"));

        let result = to_logfmt_impl(map);
        assert!(result.contains("level=info") || result.contains("level=\"info\""));
        assert!(result.contains("msg=test") || result.contains("msg=\"test\""));
    }

    #[test]
    fn test_to_kv_basic() {
        let mut map = rhai::Map::new();
        map.insert("a".into(), Dynamic::from("1"));
        map.insert("b".into(), Dynamic::from("2"));

        let result = to_kv_impl(map, None, "=");
        assert!(result.contains("a=1") || result.contains("a=\"1\""));
        assert!(result.contains("b=2") || result.contains("b=\"2\""));
    }

    #[test]
    fn test_sanitize_logfmt_key() {
        assert_eq!(sanitize_logfmt_key("normal_key"), "normal_key");
        assert_eq!(sanitize_logfmt_key("key with spaces"), "key_with_spaces");
        assert_eq!(sanitize_logfmt_key("key=value"), "key_value");
    }

    #[test]
    fn test_escape_cef_value() {
        assert_eq!(escape_cef_value("test|value"), "test\\|value");
        assert_eq!(escape_cef_value("test\\value"), "test\\\\value");
    }
}
