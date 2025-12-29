//! Parsing functions for various text formats.
//!
//! Provides functions for parsing URLs, emails, user agents, media types,
//! content disposition headers, key-value pairs, and JWT tokens.

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use rhai::{Array, Dynamic, Engine, Map};
use std::path::Path;
use url::Url;

/// Maximum length for parsed inputs (1MB)
const MAX_PARSE_LEN: usize = 1_048_576;

// ============================================================================
// Helper functions
// ============================================================================

fn split_semicolon_params(section: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = section.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            '\\' if in_quotes => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ';' if !in_quotes => {
                if !current.trim().is_empty() {
                    parts.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }

    parts
}

fn unescape_quoted_value(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn is_valid_http_token(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|ch| {
            matches!(
                ch,
                'A'..='Z'
                    | 'a'..='z'
                    | '0'..='9'
                    | '!' | '#' | '$' | '%' | '&' | '\'' | '*'
                    | '+' | '-' | '.' | '^' | '_' | '`' | '|' | '~'
            )
        })
}

fn percent_decode_to_vec(input: &str) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = bytes[i + 1];
                let lo = bytes[i + 2];
                let value = (hex_value(hi)? << 4) | hex_value(lo)?;
                result.push(value);
                i += 3;
            }
            b => {
                result.push(b);
                i += 1;
            }
        }

        if result.len() > MAX_PARSE_LEN {
            return None;
        }
    }
    Some(result)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn extract_version_token(ua: &str, ua_lower: &str, token: &str) -> Option<String> {
    let token_lower = token.to_lowercase();
    let start = ua_lower.find(&token_lower)? + token_lower.len();
    let mut end = ua.len();
    for (idx, ch) in ua[start..].char_indices() {
        if !matches!(ch, '0'..='9' | 'A'..='Z' | 'a'..='z' | '.' | '_' | '-') {
            end = start + idx;
            break;
        }
    }
    if end == start {
        None
    } else {
        Some(ua[start..end].to_string())
    }
}

fn capture_version_after(
    ua: &str,
    ua_lower: &str,
    token: &str,
    replace_underscores: bool,
) -> Option<String> {
    let token_lower = token.to_lowercase();
    let start = ua_lower.find(&token_lower)? + token_lower.len();
    let mut end = ua.len();
    for (idx, ch) in ua[start..].char_indices() {
        if !matches!(ch, '0'..='9' | 'A'..='Z' | 'a'..='z' | '.' | '_') {
            end = start + idx;
            break;
        }
    }
    if end == start {
        None
    } else {
        let mut value = ua[start..end].to_string();
        if replace_underscores {
            value = value.replace('_', ".");
        }
        Some(value)
    }
}

// ============================================================================
// URL Parsing
// ============================================================================

fn parse_path_only(input: &str) -> Map {
    let mut result = Map::new();

    // Split on fragment first
    let (pre_fragment, fragment) = input.split_once('#').unwrap_or((input, ""));

    // Split on query string
    let (path, query) = pre_fragment.split_once('?').unwrap_or((pre_fragment, ""));

    result.insert("path".into(), Dynamic::from(path.to_string()));

    if !query.is_empty() {
        result.insert("query".into(), Dynamic::from(query.to_string()));

        // Parse query params
        let params = parse_query_params_impl(query);
        if !params.is_empty() {
            result.insert("query_map".into(), Dynamic::from(params));
        }
    }

    if !fragment.is_empty() {
        result.insert("fragment".into(), Dynamic::from(fragment.to_string()));
    }

    result
}

fn parse_url_impl(input: &str) -> Map {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return Map::new();
    }

    // Try full URL first
    let (url_str, has_scheme) = if trimmed.contains("://") {
        (trimmed.to_string(), true)
    } else if trimmed.starts_with("//") {
        (format!("http:{}", trimmed), false)
    } else {
        // Fall back to path parsing
        if trimmed.starts_with('/') || trimmed.contains('?') {
            return parse_path_only(trimmed);
        }
        return Map::new();
    };

    let parsed = match Url::parse(&url_str) {
        Ok(url) => url,
        Err(_) => return Map::new(),
    };

    if parsed.host().is_none() {
        return Map::new();
    }

    let mut result = Map::new();

    if has_scheme {
        result.insert("scheme".into(), Dynamic::from(parsed.scheme().to_string()));
    }

    if !parsed.username().is_empty() {
        result.insert("user".into(), Dynamic::from(parsed.username().to_string()));
    }

    if let Some(password) = parsed.password() {
        result.insert("pass".into(), Dynamic::from(password.to_string()));
    }

    if let Some(host) = parsed.host_str() {
        result.insert("host".into(), Dynamic::from(host.to_string()));
    }

    if let Some(port) = parsed.port() {
        result.insert("port".into(), Dynamic::from(port.to_string()));
    }

    let path = parsed.path().to_string();
    if !path.is_empty() {
        result.insert("path".into(), Dynamic::from(path));
    }

    if let Some(query) = parsed.query() {
        result.insert("query".into(), Dynamic::from(query.to_string()));

        let mut qmap = Map::new();
        for (key, value) in parsed.query_pairs() {
            let key_owned = key.into_owned();
            if !qmap.contains_key(key_owned.as_str()) {
                qmap.insert(key_owned.into(), Dynamic::from(value.into_owned()));
            }
        }
        if !qmap.is_empty() {
            result.insert("query_map".into(), Dynamic::from(qmap));
        }
    }

    if let Some(fragment) = parsed.fragment() {
        result.insert("fragment".into(), Dynamic::from(fragment.to_string()));
    }

    result
}

fn parse_query_params_impl(input: &str) -> Map {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return Map::new();
    }

    let mut result = Map::new();

    // Handle query string with or without leading '?'
    let query_str = trimmed.strip_prefix('?').unwrap_or(trimmed);

    // Parse query parameters using url::form_urlencoded
    for (key, value) in url::form_urlencoded::parse(query_str.as_bytes()) {
        let key_owned = key.into_owned();
        // Only keep first occurrence of each key
        if !result.contains_key(key_owned.as_str()) {
            result.insert(key_owned.into(), Dynamic::from(value.into_owned()));
        }
    }

    result
}

fn parse_path_impl(input: &str) -> Map {
    let mut map = Map::new();
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return map;
    }

    let path = Path::new(trimmed);

    map.insert("input".into(), Dynamic::from(trimmed.to_string()));
    map.insert("is_absolute".into(), Dynamic::from(path.is_absolute()));
    map.insert("is_relative".into(), Dynamic::from(path.is_relative()));
    map.insert("has_root".into(), Dynamic::from(path.has_root()));

    if let Some(parent) = path.parent() {
        let parent_str = parent.to_string_lossy().to_string();
        if !parent_str.is_empty() {
            map.insert("parent".into(), Dynamic::from(parent_str));
        }
    }

    if let Some(file_name) = path.file_name() {
        map.insert(
            "file_name".into(),
            Dynamic::from(file_name.to_string_lossy().to_string()),
        );
    }

    if let Some(stem) = path.file_stem() {
        map.insert(
            "stem".into(),
            Dynamic::from(stem.to_string_lossy().to_string()),
        );
    }

    if let Some(ext) = path.extension() {
        map.insert(
            "extension".into(),
            Dynamic::from(ext.to_string_lossy().to_string()),
        );
    }

    let mut components_array = Array::new();
    let mut prefix_value: Option<String> = None;
    let mut root_value: Option<String> = None;

    for component in path.components() {
        use std::path::Component;

        let display = component.as_os_str().to_string_lossy().to_string();
        match component {
            Component::Prefix(prefix) => {
                let value = prefix.as_os_str().to_string_lossy().to_string();
                if prefix_value.is_none() {
                    prefix_value = Some(value.clone());
                }
                components_array.push(Dynamic::from(value));
            }
            Component::RootDir => {
                if root_value.is_none() {
                    root_value = Some(display.clone());
                }
                components_array.push(Dynamic::from(display));
            }
            Component::CurDir | Component::ParentDir | Component::Normal(_) => {
                components_array.push(Dynamic::from(display));
            }
        }
    }

    if !components_array.is_empty() {
        map.insert("components".into(), Dynamic::from(components_array));
    }

    if let Some(prefix) = prefix_value {
        map.insert("prefix".into(), Dynamic::from(prefix));
    }

    if let Some(root) = root_value {
        map.insert("root".into(), Dynamic::from(root));
    }

    map
}

// ============================================================================
// Email Parsing
// ============================================================================

fn parse_email_impl(input: &str) -> Map {
    fn is_allowed_unquoted_local_char(ch: char) -> bool {
        matches!(
            ch,
            'A'..='Z'
                | 'a'..='z'
                | '0'..='9'
                | '!' | '#' | '$' | '%' | '&' | '\'' | '*'
                | '+' | '-' | '/' | '=' | '?' | '^' | '_' | '`' | '{' | '|' | '}'
                | '~'
        )
    }

    fn parse_quoted_local(local: &str) -> Option<String> {
        if !local.starts_with('"') || !local.ends_with('"') || local.len() < 2 {
            return None;
        }

        let mut result = String::with_capacity(local.len() - 2);
        let mut chars = local[1..local.len() - 1].chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                if let Some(escaped) = chars.next() {
                    result.push(escaped);
                } else {
                    return None;
                }
            } else if ch == '"' {
                return None;
            } else {
                result.push(ch);
            }
        }
        Some(result)
    }

    fn parse_unquoted_local(local: &str) -> Option<String> {
        if local.is_empty() || local.starts_with('.') || local.ends_with('.') {
            return None;
        }

        let mut prev_dot = false;
        for ch in local.chars() {
            if ch == '.' {
                if prev_dot {
                    return None;
                }
                prev_dot = true;
                continue;
            }

            if ch.is_ascii() && is_allowed_unquoted_local_char(ch) {
                prev_dot = false;
                continue;
            }

            return None;
        }

        if prev_dot {
            return None;
        }

        Some(local.to_string())
    }

    fn is_valid_domain(domain: &str) -> bool {
        if domain.is_empty()
            || domain.len() > MAX_PARSE_LEN
            || domain.starts_with('.')
            || domain.ends_with('.')
        {
            return false;
        }

        for label in domain.split('.') {
            if label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            {
                return false;
            }
        }
        true
    }

    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return Map::new();
    }

    let mut splitter = trimmed.split('@');
    let local_raw = splitter.next().unwrap_or("");
    let domain = match splitter.next() {
        Some(value) => value,
        None => return Map::new(),
    };

    if splitter.next().is_some() {
        return Map::new();
    }

    if domain.is_empty() || local_raw.is_empty() {
        return Map::new();
    }

    if !is_valid_domain(domain) {
        return Map::new();
    }

    let local = if local_raw.starts_with('"') {
        parse_quoted_local(local_raw)
    } else {
        if local_raw.contains(char::is_whitespace) {
            return Map::new();
        }
        parse_unquoted_local(local_raw)
    };

    let local = match local {
        Some(value) => value,
        None => return Map::new(),
    };

    let mut map = Map::new();
    map.insert("local".into(), Dynamic::from(local));
    map.insert("domain".into(), Dynamic::from(domain.to_string()));
    map
}

// ============================================================================
// User Agent Parsing
// ============================================================================

fn parse_user_agent_impl(input: &str) -> Map {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return Map::new();
    }

    let ua_lower = trimmed.to_lowercase();
    let mut result = Map::new();

    let mut agent_family: Option<String> = None;
    let mut agent_version: Option<String> = None;

    let candidate_agents: &[(&str, &str)] = &[
        ("curl", "curl/"),
        ("wget", "wget/"),
        ("okhttp", "okhttp/"),
        ("Go-http-client", "go-http-client/"),
        ("Edge", "edge/"),
        ("Edge", "edg/"),
        ("Firefox", "firefox/"),
        ("Chrome", "chrome/"),
        ("Safari", "version/"),
    ];

    for (family, token) in candidate_agents {
        if let Some(version) = extract_version_token(trimmed, &ua_lower, token) {
            if *family == "Safari"
                && (!ua_lower.contains("safari/") || ua_lower.contains("chrome/"))
            {
                continue;
            }
            agent_family = Some(family.to_string());
            agent_version = Some(version);
            break;
        }
    }

    if agent_family.is_none() {
        if ua_lower.contains("mozilla/") {
            agent_family = Some("Mozilla".to_string());
        } else if ua_lower.contains("okhttp") {
            agent_family = Some("okhttp".to_string());
        }
    }

    if let Some(family) = agent_family.clone() {
        result.insert("agent_family".into(), Dynamic::from(family));
    }
    if let Some(version) = agent_version.clone() {
        if !version.is_empty() {
            result.insert("agent_version".into(), Dynamic::from(version));
        }
    }

    let mut os_family: Option<String> = None;
    let mut os_version: Option<String> = None;

    if ua_lower.contains("windows nt ") {
        if let Some(version) = capture_version_after(trimmed, &ua_lower, "windows nt ", false) {
            os_family = Some("Windows".to_string());
            os_version = Some(version);
        }
    } else if ua_lower.contains("android ") {
        if let Some(version) = capture_version_after(trimmed, &ua_lower, "android ", false) {
            os_family = Some("Android".to_string());
            os_version = Some(version);
        } else {
            os_family = Some("Android".to_string());
        }
    } else if ua_lower.contains("cpu iphone os ") {
        if let Some(version) = capture_version_after(trimmed, &ua_lower, "cpu iphone os ", true) {
            os_family = Some("iOS".to_string());
            os_version = Some(version);
        }
    } else if ua_lower.contains("iphone os ") {
        if let Some(version) = capture_version_after(trimmed, &ua_lower, "iphone os ", true) {
            os_family = Some("iOS".to_string());
            os_version = Some(version);
        }
    } else if ua_lower.contains("cpu os ") && ua_lower.contains("ipad") {
        if let Some(version) = capture_version_after(trimmed, &ua_lower, "cpu os ", true) {
            os_family = Some("iOS".to_string());
            os_version = Some(version);
        }
    } else if ua_lower.contains("mac os x ") {
        if let Some(version) = capture_version_after(trimmed, &ua_lower, "mac os x ", true) {
            os_family = Some("macOS".to_string());
            os_version = Some(version);
        } else {
            os_family = Some("macOS".to_string());
        }
    } else if ua_lower.contains("linux") {
        os_family = Some("Linux".to_string());
    }

    if let Some(family) = os_family.clone() {
        result.insert("os_family".into(), Dynamic::from(family));
    }
    if let Some(version) = os_version.clone() {
        if !version.is_empty() {
            result.insert("os_version".into(), Dynamic::from(version));
        }
    }

    let mut device: Option<String> = None;
    let lower = ua_lower.as_str();
    if lower.contains("bot")
        || lower.contains("spider")
        || lower.contains("crawler")
        || lower.contains("googlebot")
        || lower.contains("bingbot")
        || matches!(
            agent_family.as_deref(),
            Some("curl" | "wget" | "okhttp" | "Go-http-client")
        )
    {
        device = Some("Bot".to_string());
    } else if lower.contains("ipad") || lower.contains("tablet") {
        device = Some("Tablet".to_string());
    } else if lower.contains("mobile") || lower.contains("iphone") {
        device = Some("Mobile".to_string());
    } else if matches!(os_family.as_deref(), Some("Windows" | "macOS" | "Linux")) {
        device = Some("Desktop".to_string());
    }

    if let Some(device_value) = device {
        result.insert("device".into(), Dynamic::from(device_value));
    }

    if result.is_empty() {
        Map::new()
    } else {
        result
    }
}

// ============================================================================
// Media Type Parsing
// ============================================================================

fn parse_media_type_impl(input: &str) -> Map {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN || trimmed.contains(',') {
        return Map::new();
    }

    let mut iter = trimmed.splitn(2, ';');
    let type_subtype = iter.next().unwrap_or("").trim();
    if type_subtype.is_empty() {
        return Map::new();
    }

    let mut type_parts = type_subtype.splitn(2, '/');
    let r#type = type_parts.next().unwrap_or("").trim();
    let subtype = type_parts.next().unwrap_or("").trim();

    if !is_valid_http_token(r#type) || !is_valid_http_token(subtype) {
        return Map::new();
    }

    let type_lower = r#type.to_lowercase();
    let subtype_lower = subtype.to_lowercase();

    let mut result = Map::new();
    result.insert("type".into(), Dynamic::from(type_lower.clone()));
    result.insert("subtype".into(), Dynamic::from(subtype_lower.clone()));

    if let Some(dot_pos) = subtype_lower.find('.') {
        if dot_pos > 0 {
            let tree = &subtype_lower[..dot_pos];
            if !tree.is_empty() {
                result.insert("tree".into(), Dynamic::from(tree.to_string()));
            }
        }
    }

    if let Some(plus_pos) = subtype_lower.rfind('+') {
        if plus_pos + 1 < subtype_lower.len() {
            let suffix = &subtype_lower[plus_pos + 1..];
            if !suffix.is_empty() && is_valid_http_token(suffix) {
                result.insert("suffix".into(), Dynamic::from(suffix.to_string()));
            }
        }
    }

    let mut params = Map::new();
    if let Some(rest) = iter.next() {
        for param in split_semicolon_params(rest) {
            let mut kv = param.splitn(2, '=');
            let key = kv.next().unwrap_or("").trim();
            let value_raw = kv.next().unwrap_or("").trim();
            if key.is_empty() || !is_valid_http_token(key) {
                continue;
            }
            let key_lower = key.to_lowercase();
            if params.contains_key(key_lower.as_str()) {
                continue;
            }

            let value =
                if value_raw.starts_with('"') && value_raw.ends_with('"') && value_raw.len() >= 2 {
                    unescape_quoted_value(&value_raw[1..value_raw.len() - 1])
                } else {
                    value_raw.to_string()
                };

            params.insert(key_lower.into(), Dynamic::from(value));
        }
    }

    result.insert("params".into(), Dynamic::from(params));
    result
}

fn parse_content_disposition_impl(input: &str) -> Map {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_PARSE_LEN {
        return Map::new();
    }

    let mut iter = trimmed.splitn(2, ';');
    let disposition = iter.next().unwrap_or("").trim();
    if disposition.is_empty() || !is_valid_http_token(disposition) {
        return Map::new();
    }

    let mut params = Map::new();
    let mut filename_regular: Option<String> = None;
    let mut filename_star: Option<String> = None;

    if let Some(rest) = iter.next() {
        for param in split_semicolon_params(rest) {
            let mut kv = param.splitn(2, '=');
            let key = kv.next().unwrap_or("").trim();
            if key.is_empty() {
                continue;
            }
            let key_lower = key.to_lowercase();
            let raw_value = kv.next().unwrap_or("").trim();

            if !params.contains_key(key_lower.as_str()) {
                let value = if raw_value.starts_with('"')
                    && raw_value.ends_with('"')
                    && raw_value.len() >= 2
                {
                    unescape_quoted_value(&raw_value[1..raw_value.len() - 1])
                } else {
                    raw_value.to_string()
                };
                params.insert(key_lower.clone().into(), Dynamic::from(value.clone()));

                if key_lower == "filename" && filename_regular.is_none() {
                    filename_regular = Some(value);
                } else if key_lower == "filename*" && filename_star.is_none() {
                    // Handle RFC 5987 encoded filename
                    if let Some(quote_pos) = raw_value.find("''") {
                        let encoded = &raw_value[quote_pos + 2..];
                        if let Some(decoded) = percent_decode_to_vec(encoded) {
                            if let Ok(s) = String::from_utf8(decoded) {
                                filename_star = Some(s);
                            }
                        }
                    }
                }
            }
        }
    }

    let mut result = Map::new();
    result.insert(
        "disposition".into(),
        Dynamic::from(disposition.to_lowercase()),
    );
    result.insert("params".into(), Dynamic::from(params));

    // filename* takes precedence over filename
    if let Some(name) = filename_star.or(filename_regular) {
        if !name.is_empty() {
            result.insert("filename".into(), Dynamic::from(name));
        }
    }

    result
}

// ============================================================================
// Key-Value Parsing
// ============================================================================

fn parse_kv_impl(text: &str, sep: Option<&str>, kv_sep: &str) -> Map {
    let mut map = Map::new();

    // Split by separator or whitespace
    let pairs: Vec<&str> = if let Some(separator) = sep {
        text.split(separator).collect()
    } else {
        // Split by any whitespace
        text.split_whitespace().collect()
    };

    for pair in pairs {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }

        // Find the key-value separator
        if let Some(kv_pos) = pair.find(kv_sep) {
            let key = pair[..kv_pos].trim();
            let value = pair[kv_pos + kv_sep.len()..].trim();

            if !key.is_empty() {
                map.insert(key.into(), Dynamic::from(value.to_string()));
            }
        }
        // Skip tokens without separator
    }

    map
}

// ============================================================================
// JWT Parsing
// ============================================================================

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

// ============================================================================
// Registration
// ============================================================================

/// Register all parsing functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("parse_url", parse_url_impl);
    engine.register_fn("parse_query_params", parse_query_params_impl);
    engine.register_fn("parse_path", parse_path_impl);
    engine.register_fn("parse_email", parse_email_impl);
    engine.register_fn("parse_user_agent", parse_user_agent_impl);
    engine.register_fn("parse_media_type", parse_media_type_impl);
    engine.register_fn("parse_content_disposition", parse_content_disposition_impl);
    engine.register_fn("parse_jwt", parse_jwt_impl);

    // Parse key-value pairs from a string
    engine.register_fn("parse_kv", |text: &str| -> Map {
        parse_kv_impl(text, None, "=")
    });

    engine.register_fn("parse_kv", |text: &str, sep: &str| -> Map {
        parse_kv_impl(text, Some(sep), "=")
    });

    engine.register_fn("parse_kv", |text: &str, _sep: (), kv_sep: &str| -> Map {
        parse_kv_impl(text, None, kv_sep)
    });

    engine.register_fn("parse_kv", |text: &str, sep: &str, kv_sep: &str| -> Map {
        parse_kv_impl(text, Some(sep), kv_sep)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Scope;

    #[test]
    fn test_parse_url() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "url",
            "https://user:pass@example.com:8080/path?query=1#frag",
        );

        let result: Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(url)"#)
            .unwrap();

        assert_eq!(
            result.get("scheme").unwrap().clone().into_string().unwrap(),
            "https"
        );
        assert_eq!(
            result.get("host").unwrap().clone().into_string().unwrap(),
            "example.com"
        );
        assert_eq!(
            result.get("port").unwrap().clone().into_string().unwrap(),
            "8080"
        );
    }

    #[test]
    fn test_parse_email() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("email", "user@example.com");

        let result: Map = engine
            .eval_with_scope(&mut scope, r#"parse_email(email)"#)
            .unwrap();

        assert_eq!(
            result.get("local").unwrap().clone().into_string().unwrap(),
            "user"
        );
        assert_eq!(
            result.get("domain").unwrap().clone().into_string().unwrap(),
            "example.com"
        );
    }

    #[test]
    fn test_parse_jwt() {
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
    }

    #[test]
    fn test_parse_kv() {
        let mut engine = Engine::new();
        register_functions(&mut engine);

        let result: Map = engine.eval(r#"parse_kv("name=alice age=25")"#).unwrap();

        assert_eq!(
            result.get("name").unwrap().clone().into_string().unwrap(),
            "alice"
        );
        assert_eq!(
            result.get("age").unwrap().clone().into_string().unwrap(),
            "25"
        );
    }
}
