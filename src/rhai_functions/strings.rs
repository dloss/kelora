use crate::event::{json_to_dynamic, Event};
use crate::parsers::{CefParser, CombinedParser, LogfmtParser, SyslogParser};
use crate::pipeline::EventParser;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use once_cell::sync::Lazy;
use regex::Regex;
use rhai::{Array, Dynamic, Engine, Map};
use std::cell::RefCell;
use std::convert::TryFrom;
use std::path::Path;
use url::Url;

/// Represents a captured message with its target stream
#[derive(Debug, Clone)]
pub enum CapturedMessage {
    Stdout(String),
    Stderr(String),
}

thread_local! {
    static CAPTURED_PRINTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CAPTURED_EPRINTS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static CAPTURED_MESSAGES: RefCell<Vec<CapturedMessage>> = const { RefCell::new(Vec::new()) };
    static PARALLEL_MODE: RefCell<bool> = const { RefCell::new(false) };
    static SUPPRESS_SIDE_EFFECTS: RefCell<bool> = const { RefCell::new(false) };
}

const MAX_PARSE_LEN: usize = 1_048_576;

static LOGFMT_PARSER: Lazy<LogfmtParser> = Lazy::new(LogfmtParser::new);
static SYSLOG_PARSER: Lazy<SyslogParser> =
    Lazy::new(|| SyslogParser::new().expect("failed to initialize syslog parser"));
static CEF_PARSER: Lazy<CefParser> = Lazy::new(CefParser::new);
static COMBINED_PARSER: Lazy<CombinedParser> =
    Lazy::new(|| CombinedParser::new().expect("failed to initialize combined parser"));

const IPV4_PATTERN: &str = r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b";
const URL_PATTERN: &str = r##"https?://[^\s<>"]+[^\s<>".,;!?]"##;
const URL_DOMAIN_PATTERN: &str = r##"https?://([^/\s<>"]+)"##;
const EMAIL_PATTERN: &str = r"\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b";
const EMAIL_DOMAIN_PATTERN: &str = r##"[a-zA-Z0-9._%+-]+@([a-zA-Z0-9.-]+\.[a-zA-Z]{2,})"##;

static IPV4_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(IPV4_PATTERN).expect("failed to compile IPv4 regex"));
static URL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(URL_PATTERN).expect("failed to compile URL regex"));
static URL_DOMAIN_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(URL_DOMAIN_PATTERN).expect("failed to compile URL domain regex"));
static EMAIL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(EMAIL_PATTERN).expect("failed to compile email regex"));
static EMAIL_DOMAIN_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(EMAIL_DOMAIN_PATTERN).expect("failed to compile email domain regex"));

fn event_to_map(event: Event) -> Map {
    let mut map = Map::new();
    for (key, value) in event.fields {
        map.insert(key.into(), value);
    }
    map
}

fn parse_event_with<P>(parser: &P, line: &str) -> Map
where
    P: EventParser,
{
    parser
        .parse(line)
        .map(event_to_map)
        .unwrap_or_else(|_| Map::new())
}

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

fn edit_distance_impl(lhs: &str, rhs: &str) -> i64 {
    if lhs == rhs {
        return 0;
    }

    let lhs_chars: Vec<char> = lhs.chars().collect();
    let rhs_chars: Vec<char> = rhs.chars().collect();
    let len_lhs = lhs_chars.len();
    let len_rhs = rhs_chars.len();

    if len_lhs == 0 {
        return i64::try_from(len_rhs).unwrap_or(i64::MAX);
    }
    if len_rhs == 0 {
        return i64::try_from(len_lhs).unwrap_or(i64::MAX);
    }

    let mut prev: Vec<usize> = (0..=len_rhs).collect();
    let mut curr: Vec<usize> = vec![0; len_rhs + 1];

    for (i, &lhs_ch) in lhs_chars.iter().enumerate() {
        curr[0] = i + 1;

        for (j, &rhs_ch) in rhs_chars.iter().enumerate() {
            let cost = usize::from(lhs_ch != rhs_ch);
            let insertion = curr[j] + 1;
            let deletion = prev[j + 1] + 1;
            let substitution = prev[j] + cost;
            curr[j + 1] = insertion.min(deletion).min(substitution);
        }

        std::mem::swap(&mut prev, &mut curr);
    }

    i64::try_from(prev[len_rhs]).unwrap_or(i64::MAX)
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

fn parse_path_only(input: &str) -> Map {
    let mut result = Map::new();

    // Split on fragment first
    let (pre_fragment, fragment) = input.split_once('#').unwrap_or((input, ""));

    // Split on query string
    let (path, query) = pre_fragment.split_once('?').unwrap_or((pre_fragment, ""));

    result.insert("path".into(), Dynamic::from(path.to_string()));

    if !query.is_empty() {
        result.insert("query".into(), Dynamic::from(query.to_string()));

        // Parse query params (reuse existing function)
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
        // Only keep first occurrence of each key (matches parse_url behavior)
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
        if !matches!(ch, '0'..='9' | 'A'..='Z' | 'a'..='z' | '.' | '_' ) {
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
                }
            }

            if key_lower == "filename*" && filename_star.is_none() {
                let value = if raw_value.starts_with('"')
                    && raw_value.ends_with('"')
                    && raw_value.len() >= 2
                {
                    &raw_value[1..raw_value.len() - 1]
                } else {
                    raw_value
                };

                let apostrophe = '\'';
                let parts: Vec<&str> = value.splitn(3, apostrophe).collect();
                if parts.len() == 3 {
                    if let Some(decoded) = percent_decode_to_vec(parts[2]) {
                        let text = if parts[0].eq_ignore_ascii_case("utf-8") {
                            match String::from_utf8(decoded) {
                                Ok(value) => value,
                                Err(err) => String::from_utf8_lossy(err.as_bytes()).into_owned(),
                            }
                        } else {
                            String::from_utf8_lossy(&decoded).into_owned()
                        };
                        filename_star = Some(text);
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

    if let Some(name) = filename_star.or(filename_regular) {
        if !name.is_empty() {
            result.insert("filename".into(), Dynamic::from(name));
        }
    }

    result
}

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

fn parse_syslog_impl(line: &str) -> Map {
    parse_event_with(&*SYSLOG_PARSER, line)
}

fn parse_cef_impl(line: &str) -> Map {
    parse_event_with(&*CEF_PARSER, line)
}

fn parse_logfmt_impl(line: &str) -> Map {
    parse_event_with(&*LOGFMT_PARSER, line)
}

fn parse_combined_impl(line: &str) -> Map {
    parse_event_with(&*COMBINED_PARSER, line)
}

/// Capture a print statement in thread-local storage for parallel processing
pub fn capture_print(message: String) {
    CAPTURED_PRINTS.with(|prints| {
        prints.borrow_mut().push(message);
    });
}

/// Capture an eprint statement in thread-local storage for parallel processing
pub fn capture_eprint(message: String) {
    CAPTURED_EPRINTS.with(|eprints| {
        eprints.borrow_mut().push(message);
    });
}

/// Get all captured prints and clear the buffer
pub fn take_captured_prints() -> Vec<String> {
    CAPTURED_PRINTS.with(|prints| std::mem::take(&mut *prints.borrow_mut()))
}

/// Get all captured eprints and clear the buffer
pub fn take_captured_eprints() -> Vec<String> {
    CAPTURED_EPRINTS.with(|eprints| std::mem::take(&mut *eprints.borrow_mut()))
}

/// Capture a message in the ordered message system for parallel processing
pub fn capture_message(message: CapturedMessage) {
    CAPTURED_MESSAGES.with(|messages| {
        messages.borrow_mut().push(message);
    });
}

/// Capture a stdout message in the ordered system
pub fn capture_stdout(message: String) {
    capture_message(CapturedMessage::Stdout(message));
}

/// Capture a stderr message in the ordered system  
pub fn capture_stderr(message: String) {
    capture_message(CapturedMessage::Stderr(message));
}

/// Get all captured messages in order and clear the buffer
pub fn take_captured_messages() -> Vec<CapturedMessage> {
    CAPTURED_MESSAGES.with(|messages| std::mem::take(&mut *messages.borrow_mut()))
}

/// Clear captured prints without returning them
pub fn clear_captured_prints() {
    CAPTURED_PRINTS.with(|prints| {
        prints.borrow_mut().clear();
    });
}

/// Clear captured eprints without returning them
pub fn clear_captured_eprints() {
    CAPTURED_EPRINTS.with(|eprints| {
        eprints.borrow_mut().clear();
    });
}

/// Set whether we're in parallel processing mode
pub fn set_parallel_mode(enabled: bool) {
    PARALLEL_MODE.with(|mode| {
        *mode.borrow_mut() = enabled;
    });
}

/// Check if we're in parallel processing mode
pub fn is_parallel_mode() -> bool {
    PARALLEL_MODE.with(|mode| *mode.borrow())
}

/// Set whether to suppress side effects (print, eprint, etc.)
pub fn set_suppress_side_effects(suppress: bool) {
    SUPPRESS_SIDE_EFFECTS.with(|flag| {
        *flag.borrow_mut() = suppress;
    });
}

/// Check if side effects should be suppressed
pub fn is_suppress_side_effects() -> bool {
    SUPPRESS_SIDE_EFFECTS.with(|flag| *flag.borrow())
}

/// Mask IP address for privacy (replace last N octets with 'X')
fn mask_ip_impl(ip: &str, octets_to_mask: usize) -> String {
    // IPv4 pattern validation
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return ip.to_string(); // Return unchanged if not valid IPv4
    }

    // Validate each octet is numeric
    for part in &parts {
        if part.parse::<u8>().is_err() {
            return ip.to_string(); // Return unchanged if not numeric
        }
    }

    let mut result = parts.clone();
    let mask_count = octets_to_mask.clamp(1, 4);

    // Replace last N octets with 'X'
    for item in result.iter_mut().skip(4 - mask_count) {
        *item = "X";
    }

    result.join(".")
}

/// Check if IP address is in private range
fn is_private_ip_impl(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false; // Not valid IPv4
    }

    // Parse octets
    let octets: Result<Vec<u8>, _> = parts.iter().map(|s| s.parse::<u8>()).collect();
    let octets = match octets {
        Ok(o) => o,
        Err(_) => return false,
    };

    // Check private ranges
    match octets[0] {
        10 => true,                                // 10.0.0.0/8
        172 => octets[1] >= 16 && octets[1] <= 31, // 172.16.0.0/12
        192 => octets[1] == 168,                   // 192.168.0.0/16
        127 => true,                               // 127.0.0.0/8 (loopback)
        _ => false,
    }
}

/// Parse key-value pairs from a string (like logfmt format)
///
/// # Arguments
/// * `text` - The input string to parse
/// * `sep` - Optional separator between key-value pairs (default: whitespace)
/// * `kv_sep` - Separator between key and value (default: "=")
///
/// # Returns
/// A Rhai Map containing the parsed key-value pairs
fn parse_kv_impl(text: &str, sep: Option<&str>, kv_sep: &str) -> rhai::Map {
    let mut map = rhai::Map::new();

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
                map.insert(key.into(), rhai::Dynamic::from(value.to_string()));
            }
        }
        // Skip tokens without separator
    }

    map
}

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
    if text.is_empty() || nth < 0 {
        return Dynamic::from(String::new());
    }

    let nth_usize = nth as usize;
    let mut found_count = 0;
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
                    if found_count == nth_usize {
                        // This is the one we want!
                        return json_to_dynamic(&json_value);
                    }
                    found_count += 1;
                }
            }

            // Move past this structure
            i = end;
        } else {
            // No matching closing bracket found, move to next character
            i += 1;
        }
    }

    // Not found
    Dynamic::from(String::new())
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

pub fn register_functions(engine: &mut Engine) {
    // Note: print() function is now handled via engine.on_print() in engine.rs

    // Custom eprint function that captures output in parallel mode and respects suppression
    engine.register_fn("eprint", |message: rhai::Dynamic| {
        if is_suppress_side_effects() {
            // Suppress all eprint output
            return;
        }

        let msg = message.to_string();
        if is_parallel_mode() {
            // Use both old capture system (for compatibility) and new ordered system
            capture_eprint(msg.clone());
            capture_stderr(msg);
        } else {
            eprintln!("{}", msg);
        }
    });

    // Existing string functions from engine.rs
    engine.register_fn("contains", |text: &str, pattern: &str| {
        text.contains(pattern)
    });

    engine.register_fn("to_int", |text: &str| -> rhai::Dynamic {
        text.parse::<i64>()
            .map(Dynamic::from)
            .unwrap_or(Dynamic::UNIT)
    });

    engine.register_fn("to_float", |text: &str| -> rhai::Dynamic {
        text.parse::<f64>()
            .map(Dynamic::from)
            .unwrap_or(Dynamic::UNIT)
    });

    // Convert empty string to Unit for removal/filtering
    engine.register_fn("or_empty", |text: &str| -> rhai::Dynamic {
        if text.is_empty() {
            Dynamic::UNIT
        } else {
            Dynamic::from(text.to_string())
        }
    });

    // Unit overload - if already Unit, just pass through
    engine.register_fn("or_empty", |_unit: ()| -> rhai::Dynamic { Dynamic::UNIT });

    // Array overload - convert empty array to Unit
    engine.register_fn("or_empty", |arr: rhai::Array| -> rhai::Dynamic {
        if arr.is_empty() {
            Dynamic::UNIT
        } else {
            Dynamic::from(arr)
        }
    });

    // Map overload - convert empty map to Unit
    engine.register_fn("or_empty", |map: rhai::Map| -> rhai::Dynamic {
        if map.is_empty() {
            Dynamic::UNIT
        } else {
            Dynamic::from(map)
        }
    });

    engine.register_fn("slice", |s: &str, spec: &str| -> String {
        let chars: Vec<char> = s.chars().collect();
        let len = chars.len() as i32;

        if len == 0 {
            return String::new();
        }

        let parts: Vec<&str> = spec.split(':').collect();

        // Parse step first
        let step = if parts.len() > 2 && !parts[2].trim().is_empty() {
            parts[2].trim().parse::<i32>().unwrap_or(1)
        } else {
            1
        };

        if step == 0 {
            return String::new();
        }

        // Determine defaults based on step direction
        let (default_start, default_end) = if step > 0 { (0, len) } else { (len - 1, -1) };

        // Parse start
        let start = if !parts.is_empty() && !parts[0].trim().is_empty() {
            let mut s = parts[0].trim().parse::<i32>().unwrap_or(default_start);
            if s < 0 {
                s += len;
            }
            if step > 0 {
                s.clamp(0, len)
            } else {
                s.clamp(0, len - 1)
            }
        } else {
            default_start
        };

        // Parse end
        let end = if parts.len() > 1 && !parts[1].trim().is_empty() {
            let mut e = parts[1].trim().parse::<i32>().unwrap_or(default_end);
            if e < 0 {
                e += len;
            }
            if step > 0 {
                e.clamp(0, len)
            } else {
                e.clamp(-1, len - 1)
            }
        } else {
            default_end
        };

        let mut result = String::new();
        let mut i = start;

        if step > 0 {
            while i < end {
                if i >= 0 && i < len {
                    result.push(chars[i as usize]);
                }
                i += step;
            }
        } else {
            while i > end {
                if i >= 0 && i < len {
                    result.push(chars[i as usize]);
                }
                i += step;
            }
        }

        result
    });

    // String processing functions (literal string matching, not regex)
    engine.register_fn("after", |text: &str, substring: &str| -> String {
        if let Some(pos) = text.find(substring) {
            text[pos + substring.len()..].to_string()
        } else {
            String::new()
        }
    });

    engine.register_fn("after", |text: &str, substring: &str, nth: i64| -> String {
        if nth == 0 {
            return String::new();
        }

        // Collect all match positions
        let mut positions = Vec::new();
        let mut start = 0;
        while let Some(pos) = text[start..].find(substring) {
            positions.push(start + pos);
            start += pos + substring.len();
        }

        if positions.is_empty() {
            return String::new();
        }

        // Handle negative indexing (from the end)
        let idx = if nth < 0 {
            let abs_nth = (-nth) as usize;
            if abs_nth > positions.len() {
                return String::new();
            }
            positions.len() - abs_nth
        } else {
            let nth_usize = nth as usize;
            if nth_usize < 1 || nth_usize > positions.len() {
                return String::new();
            }
            nth_usize - 1 // Convert to 0-indexed
        };

        let pos = positions[idx];
        text[pos + substring.len()..].to_string()
    });

    engine.register_fn("before", |text: &str, substring: &str| -> String {
        if let Some(pos) = text.find(substring) {
            text[..pos].to_string()
        } else {
            String::new()
        }
    });

    engine.register_fn(
        "before",
        |text: &str, substring: &str, nth: i64| -> String {
            if nth == 0 {
                return String::new();
            }

            // Collect all match positions
            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(substring) {
                positions.push(start + pos);
                start += pos + substring.len();
            }

            if positions.is_empty() {
                return String::new();
            }

            // Handle negative indexing (from the end)
            let idx = if nth < 0 {
                let abs_nth = (-nth) as usize;
                if abs_nth > positions.len() {
                    return String::new();
                }
                positions.len() - abs_nth
            } else {
                let nth_usize = nth as usize;
                if nth_usize < 1 || nth_usize > positions.len() {
                    return String::new();
                }
                nth_usize - 1 // Convert to 0-indexed
            };

            let pos = positions[idx];
            text[..pos].to_string()
        },
    );

    engine.register_fn(
        "between",
        |text: &str, start_substring: &str, end_substring: &str| -> String {
            if let Some(start_pos) = text.find(start_substring) {
                let start_idx = start_pos + start_substring.len();
                let remainder = &text[start_idx..];

                if end_substring.is_empty() {
                    // Empty end substring means "to end of string"
                    remainder.to_string()
                } else if let Some(end_pos) = remainder.find(end_substring) {
                    remainder[..end_pos].to_string()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        },
    );

    engine.register_fn(
        "between",
        |text: &str, start_substring: &str, end_substring: &str, nth: i64| -> String {
            if nth == 0 {
                return String::new();
            }

            // Collect all match positions for start substring
            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(start_substring) {
                positions.push(start + pos);
                start += pos + start_substring.len();
            }

            if positions.is_empty() {
                return String::new();
            }

            // Handle negative indexing (from the end)
            let idx = if nth < 0 {
                let abs_nth = (-nth) as usize;
                if abs_nth > positions.len() {
                    return String::new();
                }
                positions.len() - abs_nth
            } else {
                let nth_usize = nth as usize;
                if nth_usize < 1 || nth_usize > positions.len() {
                    return String::new();
                }
                nth_usize - 1 // Convert to 0-indexed
            };

            let start_pos = positions[idx];
            let start_idx = start_pos + start_substring.len();
            let remainder = &text[start_idx..];

            if end_substring.is_empty() {
                // Empty end substring means "to end of string"
                remainder.to_string()
            } else if let Some(end_pos) = remainder.find(end_substring) {
                remainder[..end_pos].to_string()
            } else {
                String::new()
            }
        },
    );

    engine.register_fn("starting_with", |text: &str, prefix: &str| -> String {
        if let Some(pos) = text.find(prefix) {
            text[pos..].to_string()
        } else {
            String::new()
        }
    });

    engine.register_fn(
        "starting_with",
        |text: &str, prefix: &str, nth: i64| -> String {
            if nth == 0 {
                return String::new();
            }

            // Collect all match positions
            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(prefix) {
                positions.push(start + pos);
                start += pos + prefix.len();
            }

            if positions.is_empty() {
                return String::new();
            }

            // Handle negative indexing (from the end)
            let idx = if nth < 0 {
                let abs_nth = (-nth) as usize;
                if abs_nth > positions.len() {
                    return String::new();
                }
                positions.len() - abs_nth
            } else {
                let nth_usize = nth as usize;
                if nth_usize < 1 || nth_usize > positions.len() {
                    return String::new();
                }
                nth_usize - 1 // Convert to 0-indexed
            };

            let pos = positions[idx];
            text[pos..].to_string()
        },
    );

    engine.register_fn("ending_with", |text: &str, suffix: &str| -> String {
        if let Some(pos) = text.rfind(suffix) {
            text[..pos + suffix.len()].to_string()
        } else {
            String::new()
        }
    });

    engine.register_fn(
        "ending_with",
        |text: &str, suffix: &str, nth: i64| -> String {
            if nth == 0 {
                return String::new();
            }

            // Collect all match positions
            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(suffix) {
                positions.push(start + pos);
                start += pos + suffix.len();
            }

            if positions.is_empty() {
                return String::new();
            }

            // Handle negative indexing (from the end)
            let idx = if nth < 0 {
                let abs_nth = (-nth) as usize;
                if abs_nth > positions.len() {
                    return String::new();
                }
                positions.len() - abs_nth
            } else {
                let nth_usize = nth as usize;
                if nth_usize < 1 || nth_usize > positions.len() {
                    return String::new();
                }
                nth_usize - 1 // Convert to 0-indexed
            };

            let pos = positions[idx];
            text[..pos + suffix.len()].to_string()
        },
    );

    // Structured parsing helpers
    engine.register_fn("parse_url", parse_url_impl);
    engine.register_fn("parse_query_params", parse_query_params_impl);
    engine.register_fn("parse_path", parse_path_impl);
    engine.register_fn("parse_email", parse_email_impl);
    engine.register_fn("parse_user_agent", parse_user_agent_impl);
    engine.register_fn("parse_media_type", parse_media_type_impl);
    engine.register_fn("parse_content_disposition", parse_content_disposition_impl);
    engine.register_fn("parse_jwt", parse_jwt_impl);
    engine.register_fn("parse_syslog", parse_syslog_impl);
    engine.register_fn("parse_cef", parse_cef_impl);
    engine.register_fn("parse_logfmt", parse_logfmt_impl);
    engine.register_fn("parse_combined", parse_combined_impl);

    // Parse key-value pairs from a string (like logfmt)
    engine.register_fn("parse_kv", |text: &str| -> rhai::Map {
        parse_kv_impl(text, None, "=")
    });

    engine.register_fn("parse_kv", |text: &str, sep: &str| -> rhai::Map {
        parse_kv_impl(text, Some(sep), "=")
    });

    engine.register_fn(
        "parse_kv",
        |text: &str, sep: &str, kv_sep: &str| -> rhai::Map {
            parse_kv_impl(text, Some(sep), kv_sep)
        },
    );

    // Allow unit type for null separator
    engine.register_fn(
        "parse_kv",
        |text: &str, _sep: (), kv_sep: &str| -> rhai::Map { parse_kv_impl(text, None, kv_sep) },
    );

    // Complementary to_<format>() functions for serialization

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

    // String case conversion functions
    engine.register_fn("lower", |text: &str| -> String { text.to_lowercase() });

    engine.register_fn("upper", |text: &str| -> String { text.to_uppercase() });

    // Python-style string methods
    engine.register_fn("is_digit", |text: &str| -> bool {
        !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
    });

    engine.register_fn("count", |text: &str, pattern: &str| -> i64 {
        if pattern.is_empty() {
            return 0;
        }
        text.matches(pattern).count() as i64
    });

    engine.register_fn("edit_distance", edit_distance_impl);

    engine.register_fn("strip", |text: &str| -> String { text.trim().to_string() });

    engine.register_fn("strip", |text: &str, chars: &str| -> String {
        let chars_to_remove: std::collections::HashSet<char> = chars.chars().collect();
        text.trim_matches(|c: char| chars_to_remove.contains(&c))
            .to_string()
    });

    engine.register_fn("lstrip", |text: &str| -> String {
        text.trim_start().to_string()
    });

    engine.register_fn("lstrip", |text: &str, chars: &str| -> String {
        let chars_to_remove: std::collections::HashSet<char> = chars.chars().collect();
        text.trim_start_matches(|c: char| chars_to_remove.contains(&c))
            .to_string()
    });

    engine.register_fn("rstrip", |text: &str| -> String {
        text.trim_end().to_string()
    });

    engine.register_fn("rstrip", |text: &str, chars: &str| -> String {
        let chars_to_remove: std::collections::HashSet<char> = chars.chars().collect();
        text.trim_end_matches(|c: char| chars_to_remove.contains(&c))
            .to_string()
    });

    engine.register_fn("clip", |text: &str| -> String {
        text.trim_start_matches(|c: char| !c.is_alphanumeric())
            .trim_end_matches(|c: char| !c.is_alphanumeric())
            .to_string()
    });

    engine.register_fn("lclip", |text: &str| -> String {
        text.trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_string()
    });

    engine.register_fn("rclip", |text: &str| -> String {
        text.trim_end_matches(|c: char| !c.is_alphanumeric())
            .to_string()
    });

    engine.register_fn("join", |separator: &str, items: rhai::Array| -> String {
        items
            .into_iter()
            .filter_map(|item| item.into_string().ok())
            .collect::<Vec<String>>()
            .join(separator)
    });

    // Overloaded variant for method syntax: array.join(separator)
    engine.register_fn("join", |items: rhai::Array, separator: &str| -> String {
        items
            .into_iter()
            .filter_map(|item| item.into_string().ok())
            .collect::<Vec<String>>()
            .join(separator)
    });

    // Regex string methods
    engine.register_fn("extract_regex", |text: &str, pattern: &str| -> String {
        match regex::Regex::new(pattern) {
            Ok(re) => {
                if let Some(captures) = re.captures(text) {
                    // Return first captured group, or whole match if no groups
                    if captures.len() > 1 {
                        captures
                            .get(1)
                            .map(|m| m.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        captures
                            .get(0)
                            .map(|m| m.as_str())
                            .unwrap_or("")
                            .to_string()
                    }
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(), // Invalid regex returns empty string
        }
    });

    engine.register_fn(
        "extract_regex",
        |text: &str, pattern: &str, group: i64| -> String {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    if let Some(captures) = re.captures(text) {
                        let group_idx = if group < 0 {
                            // Negative indices not supported, default to 0
                            0
                        } else {
                            group as usize
                        };
                        captures
                            .get(group_idx)
                            .map(|m| m.as_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        String::new()
                    }
                }
                Err(_) => String::new(), // Invalid regex returns empty string
            }
        },
    );

    engine.register_fn(
        "extract_regexes",
        |text: &str, pattern: &str| -> rhai::Array {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    let mut results = rhai::Array::new();
                    for captures in re.captures_iter(text) {
                        if captures.len() > 1 {
                            // Multiple capture groups - return array of groups
                            let groups: rhai::Array = captures
                                .iter()
                                .skip(1) // Skip full match (index 0)
                                .filter_map(|m| {
                                    m.map(|match_| Dynamic::from(match_.as_str().to_string()))
                                })
                                .collect();
                            results.push(Dynamic::from(groups));
                        } else {
                            // No capture groups - return the full match
                            if let Some(full_match) = captures.get(0) {
                                results.push(Dynamic::from(full_match.as_str().to_string()));
                            }
                        }
                    }
                    results
                }
                Err(_) => rhai::Array::new(), // Invalid regex returns empty array
            }
        },
    );

    engine.register_fn(
        "extract_regexes",
        |text: &str, pattern: &str, group: i64| -> rhai::Array {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    let mut results = rhai::Array::new();
                    let group_idx = if group < 0 {
                        // Negative indices not supported, default to 0
                        0
                    } else {
                        group as usize
                    };

                    for captures in re.captures_iter(text) {
                        if let Some(group_match) = captures.get(group_idx) {
                            results.push(Dynamic::from(group_match.as_str().to_string()));
                        }
                    }
                    results
                }
                Err(_) => rhai::Array::new(), // Invalid regex returns empty array
            }
        },
    );

    engine.register_fn(
        "extract_re_maps",
        |text: &str, pattern: &str, field_name: &str| -> rhai::Array {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    let mut results = rhai::Array::new();
                    for captures in re.captures_iter(text) {
                        let match_value = if captures.len() > 1 {
                            // Has capture groups - use first capture group
                            captures.get(1).map(|m| m.as_str()).unwrap_or("")
                        } else {
                            // No capture groups - use full match
                            captures.get(0).map(|m| m.as_str()).unwrap_or("")
                        };

                        let mut map = Map::new();
                        map.insert(field_name.into(), Dynamic::from(match_value.to_string()));
                        results.push(Dynamic::from(map));
                    }
                    results
                }
                Err(_) => rhai::Array::new(), // Invalid regex returns empty array
            }
        },
    );

    // JSON extraction methods
    engine.register_fn("extract_json", |text: &str| -> Dynamic {
        extract_json_impl(text, 0)
    });

    engine.register_fn("extract_json", |text: &str, nth: i64| -> Dynamic {
        extract_json_impl(text, nth)
    });

    engine.register_fn("extract_jsons", |text: &str| -> rhai::Array {
        extract_jsons_impl(text)
    });

    engine.register_fn("split_re", |text: &str, pattern: &str| -> rhai::Array {
        match regex::Regex::new(pattern) {
            Ok(re) => re
                .split(text)
                .map(|s| Dynamic::from(s.to_string()))
                .collect(),
            Err(_) => vec![Dynamic::from(text.to_string())], // Invalid regex returns original string
        }
    });

    engine.register_fn(
        "replace_re",
        |text: &str, pattern: &str, replacement: &str| -> String {
            match regex::Regex::new(pattern) {
                Ok(re) => re.replace_all(text, replacement).to_string(),
                Err(_) => text.to_string(), // Invalid regex returns original string
            }
        },
    );

    // Network/IP methods
    engine.register_fn("extract_ip", |text: &str| -> String {
        IPV4_REGEX
            .find(text)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    });

    engine.register_fn("extract_ip", |text: &str, nth: i64| -> String {
        if nth == 0 {
            return String::new();
        }

        let matches: Vec<_> = IPV4_REGEX.find_iter(text).collect();

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
    });

    engine.register_fn("extract_ips", |text: &str| -> rhai::Array {
        IPV4_REGEX
            .find_iter(text)
            .map(|m| Dynamic::from(m.as_str().to_string()))
            .collect()
    });

    engine.register_fn("mask_ip", |ip: &str| -> String {
        mask_ip_impl(ip, 1) // Default: mask last octet
    });

    engine.register_fn("mask_ip", |ip: &str, octets: i64| -> String {
        mask_ip_impl(ip, octets.clamp(1, 4) as usize) // Clamp between 1-4
    });

    engine.register_fn("is_private_ip", |ip: &str| -> bool {
        is_private_ip_impl(ip)
    });

    engine.register_fn("extract_url", |text: &str| -> String {
        URL_REGEX
            .find(text)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    });

    engine.register_fn("extract_url", |text: &str, nth: i64| -> String {
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
    });

    engine.register_fn("extract_email", |text: &str| -> String {
        EMAIL_REGEX
            .find(text)
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    });

    engine.register_fn("extract_email", |text: &str, nth: i64| -> String {
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
    });

    engine.register_fn("extract_emails", |text: &str| -> rhai::Array {
        EMAIL_REGEX
            .find_iter(text)
            .map(|m| Dynamic::from(m.as_str().to_string()))
            .collect()
    });

    engine.register_fn("extract_domain", |text: &str| -> String {
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
    });
}

// Implementation functions for to_<format>() functions

/// Implementation for to_logfmt() function
/// Converts a Rhai Map to logfmt format string using the same logic as LogfmtFormatter
fn to_logfmt_impl(map: rhai::Map) -> String {
    use crate::event::{flatten_dynamic, FlattenStyle};

    let mut output = String::new();
    let mut first = true;

    for (key, value) in map {
        if !first {
            output.push(' ');
        }
        first = false;

        // Sanitize key for logfmt compliance
        let sanitized_key = sanitize_logfmt_key_local(&key);
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

            if is_string || needs_logfmt_quoting_local(&formatted_value) {
                format_quoted_logfmt_value_local(&formatted_value, &mut output);
            } else {
                output.push_str(&formatted_value);
            }
        } else {
            // Handle scalar values
            let string_val = value.to_string();
            if is_string {
                format_quoted_logfmt_value_local(&string_val, &mut output);
            } else {
                output.push_str(&string_val);
            }
        }
    }

    output
}

/// Implementation for to_kv() function with flexible separators
/// Mirrors the flexibility of parse_kv() function
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

/// Implementation for to_syslog() function
/// Generates RFC3164/RFC5424 syslog format
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

/// Implementation for to_cef() function
/// Generates Common Event Format (CEF) output
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

/// Implementation for to_combined() function
/// Generates Apache/NGINX combined log format
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

// Helper functions for logfmt formatting

/// Sanitize a field key to ensure logfmt compliance
/// Replaces problematic characters (spaces, tabs, newlines, carriage returns, equals) with underscores
fn sanitize_logfmt_key_local(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            ' ' | '\t' | '\n' | '\r' | '=' => '_',
            c => c,
        })
        .collect()
}

/// Check if a string value needs to be quoted per logfmt rules
fn needs_logfmt_quoting_local(value: &str) -> bool {
    // Quote values that contain spaces, tabs, newlines, quotes, equals, or are empty
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
fn escape_logfmt_string_local(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 10); // Some extra space for escapes

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
fn format_quoted_logfmt_value_local(value: &str, output: &mut String) {
    if needs_logfmt_quoting_local(value) {
        output.push('"');
        output.push_str(&escape_logfmt_string_local(value));
        output.push('"');
    } else {
        output.push_str(value);
    }
}

/// Escape CEF header field values (pipe characters)
fn escape_cef_value(value: &str) -> String {
    value.replace('|', "\\|").replace('\\', "\\\\")
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
    use rhai::Scope;

    #[test]
    fn test_after_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "hello world test");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("world")"#)
            .unwrap();
        assert_eq!(result, " test");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("missing")"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_after_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Mississippi");

        // First occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("ss", 1)"#)
            .unwrap();
        assert_eq!(result, "issippi");

        // Second occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("ss", 2)"#)
            .unwrap();
        assert_eq!(result, "ippi");

        // Last occurrence (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("ss", -1)"#)
            .unwrap();
        assert_eq!(result, "ippi");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("ss", 3)"#)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("ss", 0)"#)
            .unwrap();
        assert_eq!(result, "");

        // Pattern not found
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.after("zz", 1)"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_before_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "hello world test");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("world")"#)
            .unwrap();
        assert_eq!(result, "hello ");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("missing")"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_before_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Mississippi");

        // First occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("ss", 1)"#)
            .unwrap();
        assert_eq!(result, "Mi");

        // Second occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("ss", 2)"#)
            .unwrap();
        assert_eq!(result, "Missi");

        // Last occurrence (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("ss", -1)"#)
            .unwrap();
        assert_eq!(result, "Missi");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("ss", 3)"#)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.before("ss", 0)"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_between_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "start[content]end");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]")"#)
            .unwrap();
        assert_eq!(result, "content");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("missing", "]")"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "missing")"#)
            .unwrap();
        assert_eq!(result, "");

        // Test empty end substring - should return everything after start
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "")"#)
            .unwrap();
        assert_eq!(result, "content]end");

        scope.push("log", "ERROR: connection failed");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"log.between("ERROR: ", "")"#)
            .unwrap();
        assert_eq!(result, "connection failed");
    }

    #[test]
    fn test_between_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "[a][b][c][d]");

        // Test first occurrence (nth=1)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", 1)"#)
            .unwrap();
        assert_eq!(result, "a");

        // Test second occurrence (nth=2)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", 2)"#)
            .unwrap();
        assert_eq!(result, "b");

        // Test third occurrence (nth=3)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", 3)"#)
            .unwrap();
        assert_eq!(result, "c");

        // Test last occurrence (nth=-1)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", -1)"#)
            .unwrap();
        assert_eq!(result, "d");

        // Test second-to-last occurrence (nth=-2)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", -2)"#)
            .unwrap();
        assert_eq!(result, "c");

        // Test out of range (nth=10)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", 10)"#)
            .unwrap();
        assert_eq!(result, "");

        // Test out of range negative (nth=-10)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", -10)"#)
            .unwrap();
        assert_eq!(result, "");

        // Test nth=0
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.between("[", "]", 0)"#)
            .unwrap();
        assert_eq!(result, "");

        // Test with empty end delimiter
        scope.push(
            "log",
            "ERROR: first error | ERROR: second error | ERROR: third error",
        );
        let result: String = engine
            .eval_with_scope(&mut scope, r#"log.between("ERROR: ", "", 2)"#)
            .unwrap();
        assert_eq!(result, "second error | ERROR: third error");

        // Test when start delimiter not found
        let result: String = engine
            .eval_with_scope(&mut scope, r#"log.between("MISSING", " |", 1)"#)
            .unwrap();
        assert_eq!(result, "");

        // Test when end delimiter not found after nth start
        scope.push("brackets", "[start<end");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"brackets.between("[", ">", 1)"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_starting_with_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "hello world");

        // Test finding text at the beginning
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("hello")"#)
            .unwrap();
        assert_eq!(result, "hello world");

        // Test finding text in the middle
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("llo")"#)
            .unwrap();
        assert_eq!(result, "llo world");

        // Test finding text at the end
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("world")"#)
            .unwrap();
        assert_eq!(result, "world");

        // Test text not found
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("xyz")"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_starting_with_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "foo-bar-foo-baz-foo-end");

        // First occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("foo", 1)"#)
            .unwrap();
        assert_eq!(result, "foo-bar-foo-baz-foo-end");

        // Second occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("foo", 2)"#)
            .unwrap();
        assert_eq!(result, "foo-baz-foo-end");

        // Third occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("foo", 3)"#)
            .unwrap();
        assert_eq!(result, "foo-end");

        // Last occurrence (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("foo", -1)"#)
            .unwrap();
        assert_eq!(result, "foo-end");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("foo", 4)"#)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.starting_with("foo", 0)"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_ending_with_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "hello world");

        // Test finding text at the end
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("world")"#)
            .unwrap();
        assert_eq!(result, "hello world");

        // Test finding text in the middle
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("ell")"#)
            .unwrap();
        assert_eq!(result, "hell");

        // Test finding text at the beginning
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("hello")"#)
            .unwrap();
        assert_eq!(result, "hello");

        // Test text not found
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("xyz")"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_ending_with_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "foo-bar-foo-baz-foo-end");

        // First occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("foo", 1)"#)
            .unwrap();
        assert_eq!(result, "foo");

        // Second occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("foo", 2)"#)
            .unwrap();
        assert_eq!(result, "foo-bar-foo");

        // Third occurrence
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("foo", 3)"#)
            .unwrap();
        assert_eq!(result, "foo-bar-foo-baz-foo");

        // Last occurrence (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("foo", -1)"#)
            .unwrap();
        assert_eq!(result, "foo-bar-foo-baz-foo");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("foo", 4)"#)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.ending_with("foo", 0)"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_parse_url_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "url",
            "https://user:pass@example.com:8443/path/to/page?foo=bar&baz=qux#frag",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(url)"#)
            .unwrap();

        assert_eq!(
            result.get("scheme").unwrap().clone().into_string().unwrap(),
            "https"
        );
        assert_eq!(
            result.get("user").unwrap().clone().into_string().unwrap(),
            "user"
        );
        assert_eq!(
            result.get("pass").unwrap().clone().into_string().unwrap(),
            "pass"
        );
        assert_eq!(
            result.get("host").unwrap().clone().into_string().unwrap(),
            "example.com"
        );
        assert_eq!(
            result.get("port").unwrap().clone().into_string().unwrap(),
            "8443"
        );
        assert_eq!(
            result.get("path").unwrap().clone().into_string().unwrap(),
            "/path/to/page"
        );
        assert_eq!(
            result.get("query").unwrap().clone().into_string().unwrap(),
            "foo=bar&baz=qux"
        );
        assert_eq!(
            result
                .get("fragment")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "frag"
        );
        let query_map = result
            .get("query_map")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            query_map.get("foo").unwrap().clone().into_string().unwrap(),
            "bar"
        );
        assert_eq!(
            query_map.get("baz").unwrap().clone().into_string().unwrap(),
            "qux"
        );

        scope.push("schemeless", "//example.com/path");
        let schemeless: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(schemeless)"#)
            .unwrap();
        assert!(!schemeless.contains_key("scheme"));
        assert_eq!(
            schemeless
                .get("host")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "example.com"
        );

        scope.push("dup", "https://example.com/?id=1&id=2");
        let dup_map: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(dup)"#)
            .unwrap();
        let dup_query = dup_map
            .get("query_map")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            dup_query.get("id").unwrap().clone().into_string().unwrap(),
            "1"
        );

        // Test path-only input
        scope.push("path_only", "/just/a/path");
        let path_only: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(path_only)"#)
            .unwrap();
        assert_eq!(
            path_only
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/just/a/path"
        );
        assert!(!path_only.contains_key("host"));

        // Test path with query string
        scope.push("path_query", "/api/users?id=123&name=test");
        let path_query: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(path_query)"#)
            .unwrap();
        assert_eq!(
            path_query
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/api/users"
        );
        assert_eq!(
            path_query
                .get("query")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "id=123&name=test"
        );
        let path_query_map = path_query
            .get("query_map")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            path_query_map
                .get("id")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "123"
        );

        // Test path with fragment
        scope.push("path_frag", "/page#section");
        let path_frag: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(path_frag)"#)
            .unwrap();
        assert_eq!(
            path_frag
                .get("path")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "/page"
        );
        assert_eq!(
            path_frag
                .get("fragment")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "section"
        );

        // Test truly invalid input (no scheme, no host, no path indicators)
        scope.push("invalid", "just-some-text");
        let invalid: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_url(invalid)"#)
            .unwrap();
        assert!(invalid.is_empty());
    }

    #[test]
    fn test_parse_query_params_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Test basic query string with '?'
        scope.push("query1", "?foo=bar&baz=qux&hello=world");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(query1)"#)
            .unwrap();
        assert_eq!(
            result.get("foo").unwrap().clone().into_string().unwrap(),
            "bar"
        );
        assert_eq!(
            result.get("baz").unwrap().clone().into_string().unwrap(),
            "qux"
        );
        assert_eq!(
            result.get("hello").unwrap().clone().into_string().unwrap(),
            "world"
        );

        // Test query string without leading '?'
        scope.push("query2", "id=123&name=test");
        let result2: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(query2)"#)
            .unwrap();
        assert_eq!(
            result2.get("id").unwrap().clone().into_string().unwrap(),
            "123"
        );
        assert_eq!(
            result2.get("name").unwrap().clone().into_string().unwrap(),
            "test"
        );

        // Test URL encoding
        scope.push("query3", "name=hello%20world&email=user%40example.com");
        let result3: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(query3)"#)
            .unwrap();
        assert_eq!(
            result3.get("name").unwrap().clone().into_string().unwrap(),
            "hello world"
        );
        assert_eq!(
            result3.get("email").unwrap().clone().into_string().unwrap(),
            "user@example.com"
        );

        // Test duplicate keys (first occurrence wins)
        scope.push("query4", "id=1&id=2&id=3");
        let result4: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(query4)"#)
            .unwrap();
        assert_eq!(
            result4.get("id").unwrap().clone().into_string().unwrap(),
            "1"
        );

        // Test empty value
        scope.push("query5", "key1=&key2=value");
        let result5: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(query5)"#)
            .unwrap();
        assert_eq!(
            result5.get("key1").unwrap().clone().into_string().unwrap(),
            ""
        );
        assert_eq!(
            result5.get("key2").unwrap().clone().into_string().unwrap(),
            "value"
        );

        // Test empty string
        scope.push("empty", "");
        let empty: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(empty)"#)
            .unwrap();
        assert!(empty.is_empty());

        // Test just '?'
        scope.push("just_q", "?");
        let just_q: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_query_params(just_q)"#)
            .unwrap();
        assert!(just_q.is_empty());
    }

    #[test]
    fn test_parse_path_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("path", "logs/app.log");

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_path(path)"#)
            .unwrap();

        assert_eq!(
            result.get("input").unwrap().clone().into_string().unwrap(),
            "logs/app.log"
        );
        assert!(!result.get("is_absolute").unwrap().as_bool().unwrap());
        assert!(result.get("is_relative").unwrap().as_bool().unwrap());
        assert!(!result.get("has_root").unwrap().as_bool().unwrap());
        assert_eq!(
            result.get("parent").unwrap().clone().into_string().unwrap(),
            "logs"
        );
        assert_eq!(
            result
                .get("file_name")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "app.log"
        );
        assert_eq!(
            result.get("stem").unwrap().clone().into_string().unwrap(),
            "app"
        );
        assert_eq!(
            result
                .get("extension")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "log"
        );

        let components = result
            .get("components")
            .unwrap()
            .clone()
            .into_array()
            .unwrap();
        let component_strings: Vec<String> = components
            .into_iter()
            .map(|item| item.into_string().unwrap())
            .collect();
        assert_eq!(component_strings, vec!["logs", "app.log"]);
    }

    #[test]
    fn test_parse_email_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("email", "user.name+tag@example.co.uk");

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_email(email)"#)
            .unwrap();

        assert_eq!(
            result.get("local").unwrap().clone().into_string().unwrap(),
            "user.name+tag"
        );
        assert_eq!(
            result.get("domain").unwrap().clone().into_string().unwrap(),
            "example.co.uk"
        );
        assert_eq!(result.len(), 2);

        scope.push("quoted", "\"a b\"@xn--exmpl-hra.com");
        let quoted: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_email(quoted)"#)
            .unwrap();
        assert_eq!(
            quoted.get("local").unwrap().clone().into_string().unwrap(),
            "a b"
        );
        assert_eq!(
            quoted.get("domain").unwrap().clone().into_string().unwrap(),
            "xn--exmpl-hra.com"
        );

        scope.push("invalid", "missing-at.example.com");
        let invalid: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_email(invalid)"#)
            .unwrap();
        assert!(invalid.is_empty());
    }

    #[test]
    fn test_parse_user_agent_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "ua",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/114.0.0.0 Safari/537.36",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_user_agent(ua)"#)
            .unwrap();

        assert_eq!(
            result
                .get("agent_family")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "Chrome"
        );
        assert_eq!(
            result
                .get("agent_version")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "114.0.0.0"
        );
        assert_eq!(
            result
                .get("os_family")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "macOS"
        );
        assert_eq!(
            result
                .get("os_version")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "10.15.7"
        );
        assert_eq!(
            result.get("device").unwrap().clone().into_string().unwrap(),
            "Desktop"
        );

        scope.push("bot", "curl/8.1.0");
        let bot: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_user_agent(bot)"#)
            .unwrap();
        assert_eq!(
            bot.get("agent_family")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "curl"
        );
        assert_eq!(
            bot.get("device").unwrap().clone().into_string().unwrap(),
            "Bot"
        );
    }

    #[test]
    fn test_parse_media_type_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "mt",
            "Application/vnd.api+JSON; charset=\"utf-8\"; version=1",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_media_type(mt)"#)
            .unwrap();

        assert_eq!(
            result.get("type").unwrap().clone().into_string().unwrap(),
            "application"
        );
        assert_eq!(
            result
                .get("subtype")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "vnd.api+json"
        );
        assert_eq!(
            result.get("tree").unwrap().clone().into_string().unwrap(),
            "vnd"
        );
        assert_eq!(
            result.get("suffix").unwrap().clone().into_string().unwrap(),
            "json"
        );
        let params = result
            .get("params")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            params
                .get("charset")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "utf-8"
        );
        assert_eq!(
            params
                .get("version")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "1"
        );

        scope.push("invalid_mt", "textplain");
        let invalid: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_media_type(invalid_mt)"#)
            .unwrap();
        assert!(invalid.is_empty());
    }

    #[test]
    fn test_parse_content_disposition_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "cd",
            "attachment; filename=\"resume.pdf\"; filename*=utf-8''r%C3%A9sum%C3%A9.pdf",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_content_disposition(cd)"#)
            .unwrap();

        assert_eq!(
            result
                .get("disposition")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "attachment"
        );
        assert_eq!(
            result
                .get("filename")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "résumé.pdf"
        );

        let params = result
            .get("params")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert!(params.contains_key("filename"));
        assert!(params.contains_key("filename*"));

        scope.push("bad_cd", "attachment");
        let bad: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_content_disposition(bad_cd)"#)
            .unwrap();
        assert!(!bad.is_empty());

        scope.push("empty_cd", "");
        let empty: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_content_disposition(empty_cd)"#)
            .unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_parse_jwt_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "jwt",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
             eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiYWRtaW4iOnRydWV9.\
             SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        );

        let result: rhai::Map = engine
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
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            claims.get("sub").unwrap().clone().into_string().unwrap(),
            "1234567890"
        );
        assert!(claims.get("admin").unwrap().clone().as_bool().unwrap());

        scope.push("invalid_jwt", "ab$cd.efg");
        let invalid: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_jwt(invalid_jwt)"#)
            .unwrap();
        assert!(invalid.is_empty());
    }

    #[test]
    fn test_parse_syslog_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "line",
            "<34>1 2023-10-11T22:14:15.003Z server01 app - - - Test message",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_syslog(line)"#)
            .unwrap();

        assert_eq!(result.get("pri").unwrap().as_int().unwrap(), 34);
        assert_eq!(result.get("facility").unwrap().as_int().unwrap(), 4);
        assert_eq!(result.get("severity").unwrap().as_int().unwrap(), 2);
        assert_eq!(
            result.get("host").unwrap().clone().into_string().unwrap(),
            "server01"
        );
        assert_eq!(
            result.get("prog").unwrap().clone().into_string().unwrap(),
            "app"
        );
        assert_eq!(
            result.get("msg").unwrap().clone().into_string().unwrap(),
            "Test message"
        );
    }

    #[test]
    fn test_parse_cef_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "line",
            "CEF:0|Security|threatmanager|1.0|100|worm successfully stopped|10|src=10.0.0.1 dst=2.1.2.2 spt=1232",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_cef(line)"#)
            .unwrap();

        assert_eq!(
            result.get("vendor").unwrap().clone().into_string().unwrap(),
            "Security"
        );
        assert_eq!(
            result
                .get("product")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "threatmanager"
        );
        assert_eq!(
            result.get("src").unwrap().clone().into_string().unwrap(),
            "10.0.0.1"
        );
        assert_eq!(result.get("spt").unwrap().as_int().unwrap(), 1232);
    }

    #[test]
    fn test_parse_logfmt_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("line", "level=info message=hello count=5");

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_logfmt(line)"#)
            .unwrap();

        assert_eq!(
            result.get("level").unwrap().clone().into_string().unwrap(),
            "info"
        );
        assert_eq!(result.get("count").unwrap().as_int().unwrap(), 5);
    }

    #[test]
    fn test_parse_combined_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "line",
            "192.168.1.1 - user [25/Dec/1995:10:00:00 +0000] \"GET /index.html HTTP/1.0\" 200 1234 \"http://example.com\" \"Mozilla\"",
        );

        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_combined(line)"#)
            .unwrap();

        assert_eq!(
            result.get("ip").unwrap().clone().into_string().unwrap(),
            "192.168.1.1"
        );
        assert_eq!(result.get("status").unwrap().as_int().unwrap(), 200);
        assert_eq!(
            result.get("method").unwrap().clone().into_string().unwrap(),
            "GET"
        );
        assert_eq!(
            result.get("path").unwrap().clone().into_string().unwrap(),
            "/index.html"
        );
    }

    #[test]
    fn test_parse_kv_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Test basic key=value parsing
        scope.push("text", "key1=value1 key2=value2");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(text)"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            "value1"
        );
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "value2"
        );

        // Test with custom separator
        scope.push("text2", "key1=value1,key2=value2");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(text2, ",")"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            "value1"
        );
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "value2"
        );

        // Test with custom key-value separator
        scope.push("text3", "key1:value1 key2:value2");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(text3, (), ":")"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            "value1"
        );
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "value2"
        );

        // Test with quoted values (simple - no space handling inside quotes)
        scope.push("text4", r#"key1="quoted" key2=simple"#);
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(text4)"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            "\"quoted\""
        );
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "simple"
        );

        // Test that tokens without separator are skipped
        scope.push("text5", "key1=value1 standalone key2=value2");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(text5)"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            "value1"
        );
        // "standalone" should not be in the result since it has no separator
        assert!(!result.contains_key("standalone"));
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "value2"
        );

        // Test edge cases
        scope.push("empty", "");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(empty)"#)
            .unwrap();
        assert!(result.is_empty());

        scope.push("spaces", "  key1=value1   key2=value2  ");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(spaces)"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            "value1"
        );
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "value2"
        );

        // Test with empty values
        scope.push("empty_vals", "key1= key2=value2");
        let result: rhai::Map = engine
            .eval_with_scope(&mut scope, r#"parse_kv(empty_vals)"#)
            .unwrap();
        assert_eq!(
            result.get("key1").unwrap().clone().into_string().unwrap(),
            ""
        );
        assert_eq!(
            result.get("key2").unwrap().clone().into_string().unwrap(),
            "value2"
        );
    }

    #[test]
    fn test_lower_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Hello World");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.lower()"#)
            .unwrap();
        assert_eq!(result, "hello world");

        scope.push("mixed", "MiXeD cAsE");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"mixed.lower()"#)
            .unwrap();
        assert_eq!(result, "mixed case");
    }

    #[test]
    fn test_upper_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Hello World");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.upper()"#)
            .unwrap();
        assert_eq!(result, "HELLO WORLD");

        scope.push("mixed", "MiXeD cAsE");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"mixed.upper()"#)
            .unwrap();
        assert_eq!(result, "MIXED CASE");
    }

    #[test]
    fn test_is_digit_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("digits", "12345");
        scope.push("mixed", "123abc");
        scope.push("empty", "");
        scope.push("letters", "abcde");

        let result: bool = engine
            .eval_with_scope(&mut scope, r#"digits.is_digit()"#)
            .unwrap();
        assert!(result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r#"mixed.is_digit()"#)
            .unwrap();
        assert!(!result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r#"empty.is_digit()"#)
            .unwrap();
        assert!(!result);

        let result: bool = engine
            .eval_with_scope(&mut scope, r#"letters.is_digit()"#)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_count_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "hello world hello");
        scope.push("empty", "");

        let result: i64 = engine
            .eval_with_scope(&mut scope, r#"text.count("hello")"#)
            .unwrap();
        assert_eq!(result, 2);

        let result: i64 = engine
            .eval_with_scope(&mut scope, r#"text.count("l")"#)
            .unwrap();
        assert_eq!(result, 5);

        let result: i64 = engine
            .eval_with_scope(&mut scope, r#"text.count("missing")"#)
            .unwrap();
        assert_eq!(result, 0);

        let result: i64 = engine
            .eval_with_scope(&mut scope, r#"empty.count("x")"#)
            .unwrap();
        assert_eq!(result, 0);

        let result: i64 = engine
            .eval_with_scope(&mut scope, r#"text.count("")"#)
            .unwrap();
        assert_eq!(result, 0);
    }

    #[test]
    fn test_strip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "  hello world  ");
        scope.push("custom", "###hello world###");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.strip()"#)
            .unwrap();
        assert_eq!(result, "hello world");

        let result: String = engine
            .eval_with_scope(&mut scope, r##"custom.strip("#")"##)
            .unwrap();
        assert_eq!(result, "hello world");

        scope.push("mixed", "  ##hello world##  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"mixed.strip(" #")"##)
            .unwrap();
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_lstrip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Default whitespace stripping
        scope.push("text", "  hello world  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.lstrip()"#)
            .unwrap();
        assert_eq!(result, "hello world  ");

        // Custom character stripping
        scope.push("custom", "###hello world###");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"custom.lstrip("#")"##)
            .unwrap();
        assert_eq!(result, "hello world###");

        // Mixed characters
        scope.push("mixed", "  ##hello world##  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"mixed.lstrip(" #")"##)
            .unwrap();
        assert_eq!(result, "hello world##  ");

        // Already clean on left
        scope.push("clean", "hello world  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"clean.lstrip()"#)
            .unwrap();
        assert_eq!(result, "hello world  ");

        // Empty result
        scope.push("spaces", "   ");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"spaces.lstrip()"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_rstrip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Default whitespace stripping
        scope.push("text", "  hello world  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"text.rstrip()"#)
            .unwrap();
        assert_eq!(result, "  hello world");

        // Custom character stripping
        scope.push("custom", "###hello world###");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"custom.rstrip("#")"##)
            .unwrap();
        assert_eq!(result, "###hello world");

        // Mixed characters
        scope.push("mixed", "  ##hello world##  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"mixed.rstrip(" #")"##)
            .unwrap();
        assert_eq!(result, "  ##hello world");

        // Already clean on right
        scope.push("clean", "  hello world");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"clean.rstrip()"#)
            .unwrap();
        assert_eq!(result, "  hello world");

        // Empty result
        scope.push("spaces", "   ");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"spaces.rstrip()"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_clip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Basic punctuation removal
        scope.push("parens", "(error)");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"parens.clip()"#)
            .unwrap();
        assert_eq!(result, "error");

        // Mixed symbols
        scope.push("brackets", "[WARNING]!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"brackets.clip()"#)
            .unwrap();
        assert_eq!(result, "WARNING");

        // Empty result - all non-alnum
        scope.push("symbols", "!!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"symbols.clip()"#)
            .unwrap();
        assert_eq!(result, "");

        // Already clean
        scope.push("clean", "abc123");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"clean.clip()"#)
            .unwrap();
        assert_eq!(result, "abc123");

        // Unicode support
        scope.push("unicode", "¡Hola!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unicode.clip()"#)
            .unwrap();
        assert_eq!(result, "Hola");

        // Unicode non-Latin alphanumeric
        scope.push("japanese", "[日本語]");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"japanese.clip()"#)
            .unwrap();
        assert_eq!(result, "日本語");

        // Mixed whitespace and symbols
        scope.push("mixed", "  [ERROR]!!  ");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"mixed.clip()"#)
            .unwrap();
        assert_eq!(result, "ERROR");

        // Preserves internal non-alnum
        scope.push("internal", "!hello-world!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"internal.clip()"#)
            .unwrap();
        assert_eq!(result, "hello-world");

        // Empty string
        scope.push("empty", "");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"empty.clip()"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_lclip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Basic left clip
        scope.push("parens", "(error)");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"parens.lclip()"#)
            .unwrap();
        assert_eq!(result, "error)");

        // Only left side cleaned
        scope.push("brackets", "!!![WARNING]!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"brackets.lclip()"#)
            .unwrap();
        assert_eq!(result, "WARNING]!!");

        // Already clean on left
        scope.push("clean", "abc123!!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"clean.lclip()"#)
            .unwrap();
        assert_eq!(result, "abc123!!!");

        // Unicode
        scope.push("unicode", "¡Hola!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unicode.lclip()"#)
            .unwrap();
        assert_eq!(result, "Hola!");

        // All non-alnum
        scope.push("symbols", "!!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"symbols.lclip()"#)
            .unwrap();
        assert_eq!(result, "");

        // Empty string
        scope.push("empty", "");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"empty.lclip()"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_rclip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Basic right clip
        scope.push("parens", "(error)");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"parens.rclip()"#)
            .unwrap();
        assert_eq!(result, "(error");

        // Only right side cleaned
        scope.push("brackets", "!!![WARNING]!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"brackets.rclip()"#)
            .unwrap();
        assert_eq!(result, "!!![WARNING");

        // Already clean on right
        scope.push("clean", "!!!abc123");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"clean.rclip()"#)
            .unwrap();
        assert_eq!(result, "!!!abc123");

        // Unicode
        scope.push("unicode", "¡Hola!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unicode.rclip()"#)
            .unwrap();
        assert_eq!(result, "¡Hola");

        // All non-alnum
        scope.push("symbols", "!!!");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"symbols.rclip()"#)
            .unwrap();
        assert_eq!(result, "");

        // Empty string
        scope.push("empty", "");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"empty.rclip()"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_join_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Test original syntax: separator.join(array)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"",".join(["a", "b", "c"])"#)
            .unwrap();
        assert_eq!(result, "a,b,c");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"" ".join(["hello", "world"])"#)
            .unwrap();
        assert_eq!(result, "hello world");

        let result: String = engine
            .eval_with_scope(&mut scope, r#""-".join(["one"])"#)
            .unwrap();
        assert_eq!(result, "one");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"",".join([])"#)
            .unwrap();
        assert_eq!(result, "");

        // Test with mixed types (non-strings filtered out)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"",".join(["a", 123, "b"])"#)
            .unwrap();
        assert_eq!(result, "a,b");

        // Test new method syntax: array.join(separator)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"["a", "b", "c"].join(",")"#)
            .unwrap();
        assert_eq!(result, "a,b,c");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"["hello", "world"].join(" ")"#)
            .unwrap();
        assert_eq!(result, "hello world");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"["one"].join("-")"#)
            .unwrap();
        assert_eq!(result, "one");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"[].join(",")"#)
            .unwrap();
        assert_eq!(result, "");

        // Test method syntax with mixed types (non-strings filtered out)
        let result: String = engine
            .eval_with_scope(&mut scope, r#"["a", 123, "b"].join(",")"#)
            .unwrap();
        assert_eq!(result, "a,b");
    }

    #[test]
    fn test_extract_regex_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "user=alice status=200");

        // Extract with capture group
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_regex("user=(\\w+)")"##)
            .unwrap();
        assert_eq!(result, "alice");

        // Extract without capture group (returns full match)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_regex("\\d+")"##)
            .unwrap();
        assert_eq!(result, "200");

        // No match
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_regex("missing")"##)
            .unwrap();
        assert_eq!(result, "");

        // Invalid regex
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_regex("[")"##)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_regex_with_group_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "user=alice status=200 level=info");

        // Extract specific groups from complex pattern
        let result: String = engine
            .eval_with_scope(
                &mut scope,
                r##"text.extract_regex("user=(\\w+).*status=(\\d+)", 0)"##,
            )
            .unwrap();
        assert_eq!(result, "user=alice status=200"); // Full match (group 0)

        let result: String = engine
            .eval_with_scope(
                &mut scope,
                r##"text.extract_regex("user=(\\w+).*status=(\\d+)", 1)"##,
            )
            .unwrap();
        assert_eq!(result, "alice"); // First capture group

        let result: String = engine
            .eval_with_scope(
                &mut scope,
                r##"text.extract_regex("user=(\\w+).*status=(\\d+)", 2)"##,
            )
            .unwrap();
        assert_eq!(result, "200"); // Second capture group

        // Out of bounds group (returns empty)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_regex("user=(\\w+)", 5)"##)
            .unwrap();
        assert_eq!(result, "");

        // Negative group index (defaults to 0)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_regex("user=(\\w+)", -1)"##)
            .unwrap();
        assert_eq!(result, "user=alice");
    }

    #[test]
    fn test_extract_regexes_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "a=1 b=2 c=3");

        // Extract all with capture groups
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_regexes("(\\w+)=(\\d+)")"##)
            .unwrap();
        assert_eq!(result.len(), 3);

        // Check first match groups
        let first_match = result[0].clone().into_array().unwrap();
        assert_eq!(first_match[0].clone().into_string().unwrap(), "a");
        assert_eq!(first_match[1].clone().into_string().unwrap(), "1");

        // Extract all without capture groups (just matches)
        scope.push("numbers", "10 20 30 40");
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"numbers.extract_regexes("\\d+")"##)
            .unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].clone().into_string().unwrap(), "10");
        assert_eq!(result[3].clone().into_string().unwrap(), "40");

        // No matches
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_regexes("missing")"##)
            .unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_extract_regexes_with_group_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            "user=alice status=200 user=bob status=404 user=charlie status=500",
        );

        // Extract all values from first capture group (usernames)
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r##"text.extract_regexes("user=(\\w+).*?status=(\\d+)", 1)"##,
            )
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].clone().into_string().unwrap(), "alice");
        assert_eq!(result[1].clone().into_string().unwrap(), "bob");
        assert_eq!(result[2].clone().into_string().unwrap(), "charlie");

        // Extract all values from second capture group (status codes)
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r##"text.extract_regexes("user=(\\w+).*?status=(\\d+)", 2)"##,
            )
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].clone().into_string().unwrap(), "200");
        assert_eq!(result[1].clone().into_string().unwrap(), "404");
        assert_eq!(result[2].clone().into_string().unwrap(), "500");

        // Extract all full matches (group 0)
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_regexes("user=(\\w+)", 0)"##)
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].clone().into_string().unwrap(), "user=alice");
        assert_eq!(result[1].clone().into_string().unwrap(), "user=bob");
        assert_eq!(result[2].clone().into_string().unwrap(), "user=charlie");

        // Out of bounds group (returns empty array)
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_regexes("user=(\\w+)", 5)"##)
            .unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_extract_re_maps_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "alice@test.com and bob@example.org");

        // Extract emails without capture groups (uses full match)
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r##"text.extract_re_maps("\\w+@\\w+\\.\\w+", "email")"##,
            )
            .unwrap();
        assert_eq!(result.len(), 2);

        // Check first email map
        let first_map = result[0].clone().try_cast::<Map>().unwrap();
        assert_eq!(
            first_map
                .get("email")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "alice@test.com"
        );

        // Check second email map
        let second_map = result[1].clone().try_cast::<Map>().unwrap();
        assert_eq!(
            second_map
                .get("email")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "bob@example.org"
        );

        // Extract with capture groups (uses first capture group)
        scope.push("usertext", "user=alice status=200 user=bob status=404");
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r##"usertext.extract_re_maps("user=(\\w+)", "username")"##,
            )
            .unwrap();
        assert_eq!(result.len(), 2);

        let first_user = result[0].clone().try_cast::<Map>().unwrap();
        assert_eq!(
            first_user
                .get("username")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "alice"
        );

        let second_user = result[1].clone().try_cast::<Map>().unwrap();
        assert_eq!(
            second_user
                .get("username")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "bob"
        );

        // No matches (returns empty array)
        scope.push("nomatch", "no emails here");
        let result: rhai::Array = engine
            .eval_with_scope(
                &mut scope,
                r##"nomatch.extract_re_maps("\\w+@\\w+", "email")"##,
            )
            .unwrap();
        assert_eq!(result.len(), 0);

        // Invalid regex (returns empty array)
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_re_maps("[", "invalid")"##)
            .unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_extract_re_maps_with_emit_each() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);
        crate::rhai_functions::emit::register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Found IPs: 192.168.1.1 and 10.0.0.1");

        // Test composability with emit_each
        let result: i64 = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let ip_maps = text.extract_re_maps("\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b", "ip");
                emit_each(ip_maps)
            "##,
            )
            .unwrap();

        // Should return count of emitted events
        assert_eq!(result, 2);

        // Check that events were emitted and original suppressed
        assert!(crate::rhai_functions::emit::should_suppress_current_event());

        let emissions = crate::rhai_functions::emit::get_and_clear_pending_emissions();
        assert_eq!(emissions.len(), 2);

        assert_eq!(
            emissions[0]
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "192.168.1.1"
        );
        assert_eq!(
            emissions[1]
                .get("ip")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "10.0.0.1"
        );
    }

    #[test]
    fn test_split_re_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "one,two;three:four");

        // Split by multiple delimiters
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.split_re("[,;:]")"##)
            .unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].clone().into_string().unwrap(), "one");
        assert_eq!(result[1].clone().into_string().unwrap(), "two");
        assert_eq!(result[2].clone().into_string().unwrap(), "three");
        assert_eq!(result[3].clone().into_string().unwrap(), "four");

        // Split by whitespace
        scope.push("spaced", "hello    world\ttab\nnewline");
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"spaced.split_re("\\s+")"##)
            .unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].clone().into_string().unwrap(), "hello");
        assert_eq!(result[1].clone().into_string().unwrap(), "world");

        // Invalid regex (returns original string)
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.split_re("[")"##)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].clone().into_string().unwrap(),
            "one,two;three:four"
        );
    }

    #[test]
    fn test_replace_re_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "The year 2023 and 2024 are here");

        // Replace all years with "YEAR"
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.replace_re("\\d{4}", "YEAR")"##)
            .unwrap();
        assert_eq!(result, "The year YEAR and YEAR are here");

        // Replace with capture groups
        scope.push("emails", "Contact alice@example.com or bob@test.org");
        let result: String = engine
            .eval_with_scope(
                &mut scope,
                r##"emails.replace_re("(\\w+)@(\\w+\\.\\w+)", "[$1 at $2]")"##,
            )
            .unwrap();
        assert_eq!(
            result,
            "Contact [alice at example.com] or [bob at test.org]"
        );

        // No matches (returns original)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.replace_re("nomatch", "replacement")"##)
            .unwrap();
        assert_eq!(result, "The year 2023 and 2024 are here");

        // Invalid regex (returns original)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.replace_re("[", "replacement")"##)
            .unwrap();
        assert_eq!(result, "The year 2023 and 2024 are here");
    }

    #[test]
    fn test_extract_ip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Server 192.168.1.100 responded");

        // Extract single IP
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip()"##)
            .unwrap();
        assert_eq!(result, "192.168.1.100");

        // No IP found
        scope.push("no_ip", "No IP address here");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"no_ip.extract_ip()"##)
            .unwrap();
        assert_eq!(result, "");

        // Multiple IPs, returns first
        scope.push("multi", "From 10.0.0.1 to 172.16.0.1");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"multi.extract_ip()"##)
            .unwrap();
        assert_eq!(result, "10.0.0.1");
    }

    #[test]
    fn test_extract_ip_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "From 10.0.0.1 to 192.168.1.1 via 172.16.0.1");

        // First IP
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(1)"##)
            .unwrap();
        assert_eq!(result, "10.0.0.1");

        // Second IP
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(2)"##)
            .unwrap();
        assert_eq!(result, "192.168.1.1");

        // Third IP
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(3)"##)
            .unwrap();
        assert_eq!(result, "172.16.0.1");

        // Last IP (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(-1)"##)
            .unwrap();
        assert_eq!(result, "172.16.0.1");

        // Second to last IP
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(-2)"##)
            .unwrap();
        assert_eq!(result, "192.168.1.1");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(4)"##)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_ip(0)"##)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_ips_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "From 10.0.0.1 to 172.16.0.1 via 192.168.1.1");

        // Extract all IPs
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_ips()"##)
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].clone().into_string().unwrap(), "10.0.0.1");
        assert_eq!(result[1].clone().into_string().unwrap(), "172.16.0.1");
        assert_eq!(result[2].clone().into_string().unwrap(), "192.168.1.1");

        // No IPs found
        scope.push("no_ips", "No IP addresses here");
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"no_ips.extract_ips()"##)
            .unwrap();
        assert_eq!(result.len(), 0);

        // Invalid IP-like patterns should be excluded
        scope.push("invalid", "300.400.500.600 and 192.168.1.1");
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"invalid.extract_ips()"##)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].clone().into_string().unwrap(), "192.168.1.1");
    }

    #[test]
    fn test_extract_email_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Contact alice@example.com for help");

        // Extract single email
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email()"##)
            .unwrap();
        assert_eq!(result, "alice@example.com");

        // No email found
        scope.push("no_email", "No email address here");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"no_email.extract_email()"##)
            .unwrap();
        assert_eq!(result, "");

        // Multiple emails, returns first
        scope.push("multi", "Email alice@example.com or bob@test.org");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"multi.extract_email()"##)
            .unwrap();
        assert_eq!(result, "alice@example.com");
    }

    #[test]
    fn test_extract_email_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            "Contact alice@example.com, bob@test.org, or carol@company.co.uk",
        );

        // First email
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(1)"##)
            .unwrap();
        assert_eq!(result, "alice@example.com");

        // Second email
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(2)"##)
            .unwrap();
        assert_eq!(result, "bob@test.org");

        // Third email
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(3)"##)
            .unwrap();
        assert_eq!(result, "carol@company.co.uk");

        // Last email (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(-1)"##)
            .unwrap();
        assert_eq!(result, "carol@company.co.uk");

        // Second to last email
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(-2)"##)
            .unwrap();
        assert_eq!(result, "bob@test.org");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(4)"##)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_email(0)"##)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_emails_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            "Email alice@example.com, bob@test.org, or carol@company.co.uk",
        );

        // Extract all emails
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"text.extract_emails()"##)
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(
            result[0].clone().into_string().unwrap(),
            "alice@example.com"
        );
        assert_eq!(result[1].clone().into_string().unwrap(), "bob@test.org");
        assert_eq!(
            result[2].clone().into_string().unwrap(),
            "carol@company.co.uk"
        );

        // No emails found
        scope.push("no_emails", "No email addresses here");
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"no_emails.extract_emails()"##)
            .unwrap();
        assert_eq!(result.len(), 0);

        // Various email formats
        scope.push(
            "various",
            "test.user+tag@example.com and admin_user@sub.domain.org",
        );
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r##"various.extract_emails()"##)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].clone().into_string().unwrap(),
            "test.user+tag@example.com"
        );
        assert_eq!(
            result[1].clone().into_string().unwrap(),
            "admin_user@sub.domain.org"
        );
    }

    #[test]
    fn test_mask_ip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("ip", "192.168.1.100");

        // Default masking (last octet)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "192.168.1.X");

        // Mask 2 octets
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(2)"##)
            .unwrap();
        assert_eq!(result, "192.168.X.X");

        // Mask 3 octets
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(3)"##)
            .unwrap();
        assert_eq!(result, "192.X.X.X");

        // Mask all 4 octets
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(4)"##)
            .unwrap();
        assert_eq!(result, "X.X.X.X");

        // Invalid input (returns unchanged)
        scope.push("invalid", "not.an.ip.address");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"invalid.mask_ip()"##)
            .unwrap();
        assert_eq!(result, "not.an.ip.address");

        // Out of range values get clamped
        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(0)"##)
            .unwrap();
        assert_eq!(result, "192.168.1.X"); // Clamped to minimum 1

        let result: String = engine
            .eval_with_scope(&mut scope, r##"ip.mask_ip(10)"##)
            .unwrap();
        assert_eq!(result, "X.X.X.X"); // Clamped to maximum 4
    }

    #[test]
    fn test_is_private_ip_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();

        // Private IP ranges
        scope.push("private1", "10.0.0.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private1.is_private_ip()"##)
            .unwrap();
        assert!(result);

        scope.push("private2", "172.16.0.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private2.is_private_ip()"##)
            .unwrap();
        assert!(result);

        scope.push("private3", "192.168.1.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"private3.is_private_ip()"##)
            .unwrap();
        assert!(result);

        scope.push("loopback", "127.0.0.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"loopback.is_private_ip()"##)
            .unwrap();
        assert!(result);

        // Public IP addresses
        scope.push("public1", "8.8.8.8");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public1.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        scope.push("public2", "1.1.1.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"public2.is_private_ip()"##)
            .unwrap();
        assert!(!result);

        // Edge cases for 172.x.x.x range
        scope.push("edge1", "172.15.0.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"edge1.is_private_ip()"##)
            .unwrap();
        assert!(!result); // 172.15.x.x is not in private range

        scope.push("edge2", "172.32.0.1");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"edge2.is_private_ip()"##)
            .unwrap();
        assert!(!result); // 172.32.x.x is not in private range

        // Invalid IP addresses
        scope.push("invalid", "not.an.ip");
        let result: bool = engine
            .eval_with_scope(&mut scope, r##"invalid.is_private_ip()"##)
            .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_extract_url_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Visit https://example.com/path for more info");

        // Extract URL
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url()"##)
            .unwrap();
        assert_eq!(result, "https://example.com/path");

        // HTTP URL
        scope.push("http", "Go to http://test.org/page.html");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"http.extract_url()"##)
            .unwrap();
        assert_eq!(result, "http://test.org/page.html");

        // No URL found
        scope.push("no_url", "No URL in this text");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"no_url.extract_url()"##)
            .unwrap();
        assert_eq!(result, "");

        // Complex URL with parameters
        scope.push(
            "complex",
            "API endpoint: https://api.example.com/v1/users?page=2&limit=10",
        );
        let result: String = engine
            .eval_with_scope(&mut scope, r##"complex.extract_url()"##)
            .unwrap();
        assert_eq!(result, "https://api.example.com/v1/users?page=2&limit=10");

        // Multiple URLs (returns first)
        scope.push("multi", "Visit https://first.com or https://second.com");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"multi.extract_url()"##)
            .unwrap();
        assert_eq!(result, "https://first.com");
    }

    #[test]
    fn test_extract_url_function_with_nth() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push(
            "text",
            "Visit https://first.com or https://second.com or https://third.com",
        );

        // First URL
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(1)"##)
            .unwrap();
        assert_eq!(result, "https://first.com");

        // Second URL
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(2)"##)
            .unwrap();
        assert_eq!(result, "https://second.com");

        // Third URL
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(3)"##)
            .unwrap();
        assert_eq!(result, "https://third.com");

        // Last URL (negative indexing)
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(-1)"##)
            .unwrap();
        assert_eq!(result, "https://third.com");

        // Second to last URL
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(-2)"##)
            .unwrap();
        assert_eq!(result, "https://second.com");

        // Out of range
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(4)"##)
            .unwrap();
        assert_eq!(result, "");

        // nth=0 edge case
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_url(0)"##)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_extract_domain_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Visit https://example.com/path for more info");

        // Extract domain from URL
        let result: String = engine
            .eval_with_scope(&mut scope, r##"text.extract_domain()"##)
            .unwrap();
        assert_eq!(result, "example.com");

        // Extract domain from email
        scope.push("email", "Contact us at support@test.org");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"email.extract_domain()"##)
            .unwrap();
        assert_eq!(result, "test.org");

        // URL takes precedence over email
        scope.push("both", "Visit https://example.com or email admin@test.org");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"both.extract_domain()"##)
            .unwrap();
        assert_eq!(result, "example.com");

        // No domain found
        scope.push("no_domain", "No domain in this text");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"no_domain.extract_domain()"##)
            .unwrap();
        assert_eq!(result, "");

        // Complex domain with subdomains
        scope.push("subdomain", "API: https://api.v2.example.com/endpoint");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"subdomain.extract_domain()"##)
            .unwrap();
        assert_eq!(result, "api.v2.example.com");

        // Domain with port (should be excluded)
        scope.push("port", "Connect to http://localhost:8080/api");
        let result: String = engine
            .eval_with_scope(&mut scope, r##"port.extract_domain()"##)
            .unwrap();
        assert_eq!(result, "localhost:8080");
    }

    #[test]
    fn test_unflatten_function() {
        let mut engine = rhai::Engine::new();
        crate::rhai_functions::maps::register_functions(&mut engine);

        let mut scope = Scope::new();

        // Test basic object unflattening with default separator (underscore)
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "user_name": "alice",
                    "user_age": "30",
                    "user_settings_theme": "dark"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();

        // Check nested structure
        let user_map = result
            .get("user")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            user_map.get("name").unwrap().clone().into_string().unwrap(),
            "alice"
        );
        assert_eq!(
            user_map.get("age").unwrap().clone().into_string().unwrap(),
            "30"
        );

        let settings_map = user_map
            .get("settings")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            settings_map
                .get("theme")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "dark"
        );

        // Test array unflattening with numeric indices
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "items_0_name": "first",
                    "items_1_name": "second",
                    "items_2_name": "third"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();

        let items_array = result
            .get("items")
            .unwrap()
            .clone()
            .try_cast::<rhai::Array>()
            .unwrap();
        assert_eq!(items_array.len(), 3);

        let first_item = items_array[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            first_item
                .get("name")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "first"
        );

        let second_item = items_array[1].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            second_item
                .get("name")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "second"
        );

        // Test mixed array and object structures
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "users_0_name": "alice",
                    "users_0_roles_0": "admin",
                    "users_0_roles_1": "user",
                    "users_1_name": "bob",
                    "users_1_roles_0": "user"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();

        let users_array = result
            .get("users")
            .unwrap()
            .clone()
            .try_cast::<rhai::Array>()
            .unwrap();
        assert_eq!(users_array.len(), 2);

        let alice = users_array[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            alice.get("name").unwrap().clone().into_string().unwrap(),
            "alice"
        );

        let alice_roles = alice
            .get("roles")
            .unwrap()
            .clone()
            .try_cast::<rhai::Array>()
            .unwrap();
        assert_eq!(alice_roles.len(), 2);
        assert_eq!(alice_roles[0].clone().into_string().unwrap(), "admin");
        assert_eq!(alice_roles[1].clone().into_string().unwrap(), "user");

        // Test custom separator
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "user.name": "alice",
                    "user.settings.theme": "dark"
                };
                flat.unflatten(".")
            "##,
            )
            .unwrap();

        let user_map = result
            .get("user")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            user_map.get("name").unwrap().clone().into_string().unwrap(),
            "alice"
        );

        let settings_map = user_map
            .get("settings")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            settings_map
                .get("theme")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "dark"
        );

        // Test edge cases - empty map
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{};
                flat.unflatten()
            "##,
            )
            .unwrap();
        assert!(result.is_empty());

        // Test single level keys (no unflattening needed)
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "name": "alice",
                    "age": "30"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();
        assert_eq!(
            result.get("name").unwrap().clone().into_string().unwrap(),
            "alice"
        );
        assert_eq!(
            result.get("age").unwrap().clone().into_string().unwrap(),
            "30"
        );
    }

    #[test]
    fn test_unflatten_array_edge_cases() {
        let mut engine = rhai::Engine::new();
        crate::rhai_functions::maps::register_functions(&mut engine);

        let mut scope = Scope::new();

        // Test sparse arrays (gaps in indices)
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "items_0": "first",
                    "items_2": "third",
                    "items_5": "sixth"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();

        let items_array = result
            .get("items")
            .unwrap()
            .clone()
            .try_cast::<rhai::Array>()
            .unwrap();
        assert_eq!(items_array.len(), 6); // Should extend to highest index + 1
        assert_eq!(items_array[0].clone().into_string().unwrap(), "first");
        assert!(items_array[1].is_unit()); // Gap filled with unit
        assert_eq!(items_array[2].clone().into_string().unwrap(), "third");
        assert!(items_array[3].is_unit()); // Gap
        assert!(items_array[4].is_unit()); // Gap
        assert_eq!(items_array[5].clone().into_string().unwrap(), "sixth");

        // Test array with non-numeric keys mixed in (should default to object)
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "mixed_0": "zero",
                    "mixed_name": "alice",
                    "mixed_1": "one"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();

        // Should be treated as object due to mixed keys
        let mixed_map = result
            .get("mixed")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            mixed_map.get("0").unwrap().clone().into_string().unwrap(),
            "zero"
        );
        assert_eq!(
            mixed_map
                .get("name")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "alice"
        );
        assert_eq!(
            mixed_map.get("1").unwrap().clone().into_string().unwrap(),
            "one"
        );
    }

    #[test]
    fn test_unflatten_deep_nesting() {
        let mut engine = rhai::Engine::new();
        crate::rhai_functions::maps::register_functions(&mut engine);

        let mut scope = Scope::new();

        // Test deeply nested structures
        let result: rhai::Map = engine
            .eval_with_scope(
                &mut scope,
                r##"
                let flat = #{
                    "app_config_database_host": "localhost",
                    "app_config_database_port": "5432",
                    "app_config_cache_redis_url": "redis://localhost",
                    "app_config_cache_ttl": "3600",
                    "app_features_0_name": "auth",
                    "app_features_0_enabled": "true",
                    "app_features_1_name": "logging",
                    "app_features_1_enabled": "false"
                };
                flat.unflatten()
            "##,
            )
            .unwrap();

        // Navigate the nested structure
        let app_map = result
            .get("app")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        let config_map = app_map
            .get("config")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();

        // Check database config
        let db_map = config_map
            .get("database")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            db_map.get("host").unwrap().clone().into_string().unwrap(),
            "localhost"
        );
        assert_eq!(
            db_map.get("port").unwrap().clone().into_string().unwrap(),
            "5432"
        );

        // Check cache config
        let cache_map = config_map
            .get("cache")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            cache_map.get("ttl").unwrap().clone().into_string().unwrap(),
            "3600"
        );
        let redis_map = cache_map
            .get("redis")
            .unwrap()
            .clone()
            .try_cast::<rhai::Map>()
            .unwrap();
        assert_eq!(
            redis_map.get("url").unwrap().clone().into_string().unwrap(),
            "redis://localhost"
        );

        // Check features array
        let features_array = app_map
            .get("features")
            .unwrap()
            .clone()
            .try_cast::<rhai::Array>()
            .unwrap();
        assert_eq!(features_array.len(), 2);

        let auth_feature = features_array[0].clone().try_cast::<rhai::Map>().unwrap();
        assert_eq!(
            auth_feature
                .get("name")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "auth"
        );
        assert_eq!(
            auth_feature
                .get("enabled")
                .unwrap()
                .clone()
                .into_string()
                .unwrap(),
            "true"
        );
    }

    #[test]
    fn test_to_logfmt_basic() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    level: "INFO",
                    msg: "Test message",
                    user: "alice",
                    status: 200
                };
                map.to_logfmt()
            "##,
            )
            .unwrap();

        // Check that all key-value pairs are present
        assert!(result.contains("level=INFO"));
        assert!(result.contains("msg=\"Test message\"")); // Quoted due to space
        assert!(result.contains("user=alice"));
        assert!(result.contains("status=200"));

        // Fields should be space-separated
        assert!(result.contains(" "));
    }

    #[test]
    fn test_to_logfmt_quoting() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    simple: "value",
                    spaced: "has spaces",
                    quoted: "has\"quotes",
                    equals: "has=sign",
                    empty: ""
                };
                map.to_logfmt()
            "##,
            )
            .unwrap();

        assert!(result.contains("simple=value")); // No quotes for simple value
        assert!(result.contains("spaced=\"has spaces\"")); // Quotes due to spaces
        assert!(result.contains("quoted=\"has\\\"quotes\"")); // Escaped quotes
        assert!(result.contains("equals=\"has=sign\"")); // Quotes due to equals sign
        assert!(result.contains("empty=\"\"")); // Quotes for empty string
    }

    #[test]
    fn test_to_logfmt_key_sanitization() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{};
                map["field with spaces"] = "value1";
                map["field=with=equals"] = "value2";
                map["field\twith\ttabs"] = "value3";
                map["normal_field"] = "value4";
                map.to_logfmt()
            "##,
            )
            .unwrap();

        // Keys should be sanitized
        assert!(result.contains("field_with_spaces=value1"));
        assert!(result.contains("field_with_equals=value2"));
        assert!(result.contains("field_with_tabs=value3"));
        assert!(result.contains("normal_field=value4"));
    }

    #[test]
    fn test_to_kv_basic() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    name: "alice",
                    age: 25,
                    active: true
                };
                map.to_kv()
            "##,
            )
            .unwrap();

        assert!(result.contains("name=alice"));
        assert!(result.contains("age=25"));
        assert!(result.contains("active=true"));
        assert!(result.contains(" ")); // Space separator
    }

    #[test]
    fn test_to_kv_custom_separators() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test custom field separator
        let result1: String = engine
            .eval(
                r##"
                let map = #{
                    a: "1",
                    b: "2"
                };
                map.to_kv("|")
            "##,
            )
            .unwrap();

        assert!(result1.contains("a=1|b=2") || result1.contains("b=2|a=1"));

        // Test custom field and kv separators
        let result2: String = engine
            .eval(
                r##"
                let map = #{
                    a: "1",
                    b: "2"
                };
                map.to_kv("|", ":")
            "##,
            )
            .unwrap();

        assert!(result2.contains("a:1|b:2") || result2.contains("b:2|a:1"));

        // Test null separator (should use whitespace)
        let result3: String = engine
            .eval(
                r##"
                let map = #{
                    a: "1",
                    b: "2"
                };
                map.to_kv((), ":")
            "##,
            )
            .unwrap();

        assert!(result3.contains("a:1 b:2") || result3.contains("b:2 a:1"));
    }

    #[test]
    fn test_to_syslog_basic() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    priority: "16",
                    timestamp: "Oct 24 12:34:56",
                    hostname: "server1",
                    tag: "myapp",
                    message: "Something happened"
                };
                map.to_syslog()
            "##,
            )
            .unwrap();

        assert_eq!(
            result,
            "<16>Oct 24 12:34:56 server1 myapp: Something happened"
        );
    }

    #[test]
    fn test_to_syslog_defaults() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    msg: "Test"
                };
                map.to_syslog()
            "##,
            )
            .unwrap();

        // Should use defaults
        assert!(result.starts_with("<13>")); // Default priority
        assert!(result.contains("localhost")); // Default hostname
        assert!(result.contains("kelora:")); // Default tag
        assert!(result.contains("Test")); // Message from msg field
    }

    #[test]
    fn test_to_cef_basic() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    deviceVendor: "Acme",
                    deviceProduct: "SecTool",
                    deviceVersion: "2.0",
                    signatureId: "100",
                    name: "Attack detected",
                    severity: "8",
                    src: "192.168.1.1",
                    dst: "10.0.0.1"
                };
                map.to_cef()
            "##,
            )
            .unwrap();

        assert!(result.starts_with("CEF:0|Acme|SecTool|2.0|100|Attack detected|8|"));
        assert!(result.contains("src=192.168.1.1"));
        assert!(result.contains("dst=10.0.0.1"));
    }

    #[test]
    fn test_to_cef_defaults() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    extra_field: "value"
                };
                map.to_cef()
            "##,
            )
            .unwrap();

        // Should use defaults for header fields
        assert!(result.starts_with("CEF:0|Kelora|LogAnalyzer|1.0|1|Event|5|"));
        assert!(result.contains("extra_field=value"));
    }

    #[test]
    fn test_to_combined_basic() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    ip: "192.168.1.1",
                    identity: "-",
                    user: "alice",
                    timestamp: "[25/Dec/1995:10:00:00 +0000]",
                    request: "GET /index.html HTTP/1.0",
                    status: "200",
                    bytes: "1234",
                    referer: "http://example.com/",
                    user_agent: "Mozilla/4.08"
                };
                map.to_combined()
            "##,
            )
            .unwrap();

        assert_eq!(
            result,
            r#"192.168.1.1 - alice [25/Dec/1995:10:00:00 +0000] "GET /index.html HTTP/1.0" 200 1234 "http://example.com/" "Mozilla/4.08""#
        );
    }

    #[test]
    fn test_to_combined_from_components() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    ip: "192.168.1.1",
                    method: "POST",
                    path: "/api/users",
                    protocol: "HTTP/1.1",
                    status: "201"
                };
                map.to_combined()
            "##,
            )
            .unwrap();

        // Should build request from components and use defaults
        assert!(result.contains("192.168.1.1"));
        assert!(result.contains(r#""POST /api/users HTTP/1.1""#));
        assert!(result.contains("201"));
        assert!(result.contains("- -")); // Default identity and user
        assert!(result.contains("\"-\"")); // Default referer and user agent
    }

    #[test]
    fn test_to_combined_with_request_time() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let result: String = engine
            .eval(
                r##"
                let map = #{
                    ip: "192.168.1.1",
                    method: "GET",
                    path: "/",
                    status: "200",
                    request_time: "0.123"
                };
                map.to_combined()
            "##,
            )
            .unwrap();

        // Should include request_time at the end (NGINX style)
        assert!(result.ends_with(r#" "0.123""#));
    }

    #[test]
    fn test_to_functions_roundtrip() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test logfmt roundtrip
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    level: "INFO",
                    msg: "Test message",
                    count: 42
                };
                let logfmt_string = original.to_logfmt();
                logfmt_string.parse_logfmt()
            "##,
            )
            .unwrap();

        assert_eq!(result.get("level").unwrap().to_string(), "INFO");
        assert_eq!(result.get("msg").unwrap().to_string(), "Test message");
        assert_eq!(result.get("count").unwrap().to_string(), "42");

        // Test kv roundtrip
        let result2: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    name: "alice",
                    age: "25"
                };
                let kv_string = original.to_kv();
                kv_string.parse_kv()
            "##,
            )
            .unwrap();

        assert_eq!(result2.get("name").unwrap().to_string(), "alice");
        assert_eq!(result2.get("age").unwrap().to_string(), "25");
    }

    // Invariance tests: Testing the mathematical property that parse(to(x)) = x
    // These tests ensure bidirectional compatibility between parse_* and to_* functions

    #[test]
    fn test_logfmt_parse_to_invariance() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test basic logfmt invariance: parse(to(map)) = map
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    level: "INFO",
                    message: "Test with spaces",
                    count: 42,
                    active: true,
                    ratio: 3.14
                };
                let serialized = original.to_logfmt();
                serialized.parse_logfmt()
            "##,
            )
            .unwrap();

        assert_eq!(result.get("level").unwrap().to_string(), "INFO");
        assert_eq!(
            result.get("message").unwrap().to_string(),
            "Test with spaces"
        );
        assert_eq!(result.get("count").unwrap().to_string(), "42");
        assert_eq!(result.get("active").unwrap().to_string(), "true");
        assert_eq!(result.get("ratio").unwrap().to_string(), "3.14");
    }

    #[test]
    fn test_kv_parse_to_invariance() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test kv invariance with default separators
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    name: "alice",
                    age: "25",
                    role: "admin"
                };
                let serialized = original.to_kv();
                serialized.parse_kv()
            "##,
            )
            .unwrap();

        assert_eq!(result.get("name").unwrap().to_string(), "alice");
        assert_eq!(result.get("age").unwrap().to_string(), "25");
        assert_eq!(result.get("role").unwrap().to_string(), "admin");

        // Test kv invariance with custom separators
        let result2: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    host: "server1",
                    port: "8080"
                };
                let serialized = original.to_kv("|", ":");
                serialized.parse_kv("|", ":")
            "##,
            )
            .unwrap();

        assert_eq!(result2.get("host").unwrap().to_string(), "server1");
        assert_eq!(result2.get("port").unwrap().to_string(), "8080");
    }

    #[test]
    fn test_logfmt_edge_cases_invariance() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test with problematic characters that require escaping/quoting
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    simple: "value",
                    spaced: "value with spaces",
                    quoted: "value\"with\"quotes",
                    equals: "value=with=equals",
                    empty: "",
                    newlines: "line1\nline2"
                };
                let serialized = original.to_logfmt();
                serialized.parse_logfmt()
            "##,
            )
            .unwrap();

        assert_eq!(result.get("simple").unwrap().to_string(), "value");
        assert_eq!(
            result.get("spaced").unwrap().to_string(),
            "value with spaces"
        );
        assert_eq!(
            result.get("quoted").unwrap().to_string(),
            "value\"with\"quotes"
        );
        assert_eq!(
            result.get("equals").unwrap().to_string(),
            "value=with=equals"
        );
        assert_eq!(result.get("empty").unwrap().to_string(), "");
        assert_eq!(result.get("newlines").unwrap().to_string(), "line1\nline2");
    }

    #[test]
    fn test_key_sanitization_invariance() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test that key sanitization works in roundtrip
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{};
                original["field with spaces"] = "value1";
                original["field=with=equals"] = "value2";
                original["field\twith\ttabs"] = "value3";

                let serialized = original.to_logfmt();
                serialized.parse_logfmt()
            "##,
            )
            .unwrap();

        // Keys should be sanitized consistently
        assert_eq!(
            result.get("field_with_spaces").unwrap().to_string(),
            "value1"
        );
        assert_eq!(
            result.get("field_with_equals").unwrap().to_string(),
            "value2"
        );
        assert_eq!(result.get("field_with_tabs").unwrap().to_string(), "value3");
    }

    #[test]
    fn test_triple_transformation_logfmt_kv_logfmt() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test: Map -> logfmt -> parse -> kv -> parse -> logfmt -> parse
        // Use values without spaces since parse_kv doesn't handle quoted values
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    service: "web",
                    level: "INFO",
                    code: "200",
                    user: "alice"
                };

                // Transform: Map -> logfmt -> Map -> kv -> Map -> logfmt -> Map
                let step1 = original.to_logfmt();           // Map -> logfmt string
                let step2 = step1.parse_logfmt();          // logfmt -> Map
                let step3 = step2.to_kv();                 // Map -> kv string
                let step4 = step3.parse_kv();              // kv -> Map
                let step5 = step4.to_logfmt();             // Map -> logfmt string
                step5.parse_logfmt()                       // logfmt -> Map
            "##,
            )
            .unwrap();

        // Should preserve all original data through triple transformation
        assert_eq!(result.get("service").unwrap().to_string(), "web");
        assert_eq!(result.get("level").unwrap().to_string(), "INFO");
        assert_eq!(result.get("code").unwrap().to_string(), "200");
        assert_eq!(result.get("user").unwrap().to_string(), "alice");
    }

    #[test]
    fn test_triple_transformation_kv_logfmt_kv() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Alternative triple transformation with non-space separators
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    host: "server1",
                    port: "8080",
                    status: "active"
                };

                // Transform: Map -> kv(|,:) -> Map -> logfmt -> Map -> kv(|,:) -> Map
                let step1 = original.to_kv("|", ":");      // Map -> kv string
                let step2 = step1.parse_kv("|", ":");      // kv -> Map
                let step3 = step2.to_logfmt();             // Map -> logfmt string
                let step4 = step3.parse_logfmt();          // logfmt -> Map
                let step5 = step4.to_kv("|", ":");         // Map -> kv string
                step5.parse_kv("|", ":")                   // kv -> Map
            "##,
            )
            .unwrap();

        // Should preserve all original data through triple transformation
        assert_eq!(result.get("host").unwrap().to_string(), "server1");
        assert_eq!(result.get("port").unwrap().to_string(), "8080");
        assert_eq!(result.get("status").unwrap().to_string(), "active");
    }

    #[test]
    fn test_cross_format_consistency() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test that the same data produces consistent results across formats
        // Use simple values (no spaces) to ensure kv format compatibility
        let logfmt_result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    timestamp: "2023-10-24T12:34:56Z",
                    level: "ERROR",
                    service: "auth-service",
                    user_id: "12345",
                    status_code: "401"
                };
                let logfmt_str = original.to_logfmt();
                logfmt_str.parse_logfmt()
            "##,
            )
            .unwrap();

        // Convert through kv
        let kv_result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    timestamp: "2023-10-24T12:34:56Z",
                    level: "ERROR",
                    service: "auth-service",
                    user_id: "12345",
                    status_code: "401"
                };
                let kv_str = original.to_kv();
                kv_str.parse_kv()
            "##,
            )
            .unwrap();

        // Both should preserve the same core fields
        for key in ["level", "service", "user_id", "status_code", "timestamp"] {
            assert_eq!(
                logfmt_result.get(key).unwrap().to_string(),
                kv_result.get(key).unwrap().to_string(),
                "Field '{}' differs between logfmt and kv transformations",
                key
            );
        }
    }

    #[test]
    fn test_syslog_field_preservation() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test that syslog format preserves specific fields when parsed back
        let result: String = engine
            .eval(
                r##"
                let original = #{
                    priority: "16",
                    timestamp: "Oct 24 12:34:56",
                    hostname: "web-server",
                    tag: "nginx",
                    message: "GET /api/health 200"
                };

                let syslog_line = original.to_syslog();
                // Return the generated syslog line to verify format
                syslog_line
            "##,
            )
            .unwrap();

        // Verify syslog format structure
        assert!(result.starts_with("<16>"));
        assert!(result.contains("Oct 24 12:34:56"));
        assert!(result.contains("web-server"));
        assert!(result.contains("nginx:"));
        assert!(result.contains("GET /api/health 200"));

        // Test complete format: <priority>timestamp hostname tag: message
        let expected_format = "<16>Oct 24 12:34:56 web-server nginx: GET /api/health 200";
        assert_eq!(result, expected_format);
    }

    #[test]
    fn test_cef_field_preservation() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test CEF format structure preservation
        let result: String = engine
            .eval(
                r##"
                let original = #{
                    deviceVendor: "Acme",
                    deviceProduct: "SecurityTool",
                    deviceVersion: "2.0",
                    signatureId: "100",
                    name: "Suspicious activity",
                    severity: "7",
                    src: "192.168.1.100",
                    dst: "10.0.0.1",
                    act: "blocked"
                };

                original.to_cef()
            "##,
            )
            .unwrap();

        // Verify CEF header format
        assert!(result.starts_with("CEF:0|Acme|SecurityTool|2.0|100|Suspicious activity|7|"));

        // Verify extension fields are present
        assert!(result.contains("src=192.168.1.100"));
        assert!(result.contains("dst=10.0.0.1"));
        assert!(result.contains("act=blocked"));
    }

    #[test]
    fn test_combined_log_format_consistency() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test combined log format structure
        let result: String = engine
            .eval(
                r##"
                let original = #{
                    ip: "192.168.1.1",
                    identity: "-",
                    user: "alice",
                    timestamp: "[25/Dec/1995:10:00:00 +0000]",
                    method: "GET",
                    path: "/api/users",
                    protocol: "HTTP/1.1",
                    status: "200",
                    bytes: "1234",
                    referer: "https://example.com/",
                    user_agent: "Mozilla/5.0",
                    request_time: "0.045"
                };

                original.to_combined()
            "##,
            )
            .unwrap();

        // Verify combined log format components
        assert!(result.contains("192.168.1.1"));
        assert!(result.contains("- alice"));
        assert!(result.contains("[25/Dec/1995:10:00:00 +0000]"));
        assert!(result.contains("\"GET /api/users HTTP/1.1\""));
        assert!(result.contains("200 1234"));
        assert!(result.contains("\"https://example.com/\""));
        assert!(result.contains("\"Mozilla/5.0\""));
        assert!(result.ends_with(" \"0.045\""));
    }

    #[test]
    fn test_edit_distance_function() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let distance = engine
            .eval::<i64>(r#""kitten".edit_distance("sitting")"#)
            .unwrap();
        assert_eq!(distance, 3);

        let symmetric = engine
            .eval::<i64>(r#""sitting".edit_distance("kitten")"#)
            .unwrap();
        assert_eq!(symmetric, 3);

        let same = engine
            .eval::<i64>(r#""kelora".edit_distance("kelora")"#)
            .unwrap();
        assert_eq!(same, 0);

        let empty_case = engine
            .eval::<i64>(
                r#"
                let empty = "";
                empty.edit_distance("logs")
            "#,
            )
            .unwrap();
        assert_eq!(empty_case, 4);
    }

    #[test]
    fn test_empty_and_null_handling_invariance() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test handling of empty values and null-like data
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    empty_string: "",
                    normal_field: "value"
                };

                let logfmt_str = original.to_logfmt();
                logfmt_str.parse_logfmt()
            "##,
            )
            .unwrap();

        assert_eq!(result.get("empty_string").unwrap().to_string(), "");
        assert_eq!(result.get("normal_field").unwrap().to_string(), "value");

        // Test kv handling of empty values
        let result2: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    empty: "",
                    normal: "test"
                };

                let kv_str = original.to_kv();
                kv_str.parse_kv()
            "##,
            )
            .unwrap();

        assert_eq!(result2.get("empty").unwrap().to_string(), "");
        assert_eq!(result2.get("normal").unwrap().to_string(), "test");
    }

    #[test]
    fn test_numeric_type_consistency() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test that numeric values maintain consistency through transformations
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{
                    integer: 42,
                    float: 3.14159,
                    zero: 0,
                    negative: -100
                };

                let logfmt_str = original.to_logfmt();
                logfmt_str.parse_logfmt()
            "##,
            )
            .unwrap();

        // Note: After parse, all values become strings, but should preserve numeric representation
        assert_eq!(result.get("integer").unwrap().to_string(), "42");
        assert_eq!(result.get("float").unwrap().to_string(), "3.14159");
        assert_eq!(result.get("zero").unwrap().to_string(), "0");
        assert_eq!(result.get("negative").unwrap().to_string(), "-100");
    }

    #[test]
    fn test_large_data_invariance() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test with larger datasets to ensure scalability
        let result: rhai::Map = engine
            .eval(
                r##"
                let original = #{};

                // Create a map with many fields
                for i in 0..50 {
                    let key = "field_" + i;
                    let value = "value_" + i + "_with_some_data";
                    original[key] = value;
                }

                let logfmt_str = original.to_logfmt();
                logfmt_str.parse_logfmt()
            "##,
            )
            .unwrap();

        // Verify all fields are preserved
        assert_eq!(result.len(), 50);
        assert_eq!(
            result.get("field_0").unwrap().to_string(),
            "value_0_with_some_data"
        );
        assert_eq!(
            result.get("field_25").unwrap().to_string(),
            "value_25_with_some_data"
        );
        assert_eq!(
            result.get("field_49").unwrap().to_string(),
            "value_49_with_some_data"
        );
    }
}
