use crate::event::json_to_dynamic;
use lru::LruCache;
use regex::Regex;
use rhai::{Dynamic, Engine, EvalAltResult, ImmutableString, Map};
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::num::NonZeroUsize;

const REGEX_CACHE_CAPACITY: usize = 100;

thread_local! {
    static ABSORB_STRICT: Cell<bool> = const { Cell::new(false) };
    static ABSORB_REGEX_CACHE: RefCell<LruCache<String, Regex>> = RefCell::new(LruCache::new(
        NonZeroUsize::new(REGEX_CACHE_CAPACITY).expect("regex cache capacity must be non-zero")
    ));
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("absorb_kv", absorb_kv_default);
    engine.register_fn("absorb_kv", absorb_kv_with_options);
    engine.register_fn("absorb_logfmt", absorb_logfmt_default);
    engine.register_fn("absorb_logfmt", absorb_logfmt_with_options);
    engine.register_fn("absorb_jwt", absorb_jwt_default);
    engine.register_fn("absorb_jwt", absorb_jwt_with_options);
    engine.register_fn("absorb_json", absorb_json_default);
    engine.register_fn("absorb_json", absorb_json_with_options);
    engine.register_fn("absorb_regex", absorb_regex_default);
    engine.register_fn("absorb_regex", absorb_regex_with_options);
}

pub fn set_absorb_strict(strict: bool) {
    ABSORB_STRICT.with(|flag| flag.set(strict));
}

fn is_absorb_strict() -> bool {
    ABSORB_STRICT.with(|flag| flag.get())
}

fn absorb_kv_default(event: &mut Map, field: &str) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(absorb_kv_impl(event, field, None), "absorb_kv")
}

fn absorb_kv_with_options(
    event: &mut Map,
    field: &str,
    options: Map,
) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(absorb_kv_impl(event, field, Some(&options)), "absorb_kv")
}

fn finalize_result(result: AbsorbResult, fn_name: &str) -> Result<Map, Box<EvalAltResult>> {
    if result.status == AbsorbStatus::InvalidOption && is_absorb_strict() {
        let message = result
            .error
            .clone()
            .unwrap_or_else(|| "invalid absorb option".to_string());
        return Err(format!("{}: {}", fn_name, message).into());
    }

    Ok(result.into_map())
}

fn absorb_logfmt_default(event: &mut Map, field: &str) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(absorb_logfmt_impl(event, field, None), "absorb_logfmt")
}

fn absorb_logfmt_with_options(
    event: &mut Map,
    field: &str,
    options: Map,
) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(
        absorb_logfmt_impl(event, field, Some(&options)),
        "absorb_logfmt",
    )
}

fn absorb_jwt_default(event: &mut Map, field: &str) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(absorb_jwt_impl(event, field, None), "absorb_jwt")
}

fn absorb_jwt_with_options(
    event: &mut Map,
    field: &str,
    options: Map,
) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(absorb_jwt_impl(event, field, Some(&options)), "absorb_jwt")
}

fn absorb_json_default(event: &mut Map, field: &str) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(absorb_json_impl(event, field, None), "absorb_json")
}

fn absorb_json_with_options(
    event: &mut Map,
    field: &str,
    options: Map,
) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(
        absorb_json_impl(event, field, Some(&options)),
        "absorb_json",
    )
}

fn absorb_regex_default(
    event: &mut Map,
    field: &str,
    pattern: &str,
) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(
        absorb_regex_impl(event, field, pattern, None),
        "absorb_regex",
    )
}

fn absorb_regex_with_options(
    event: &mut Map,
    field: &str,
    pattern: &str,
    options: Map,
) -> Result<Map, Box<EvalAltResult>> {
    finalize_result(
        absorb_regex_impl(event, field, pattern, Some(&options)),
        "absorb_regex",
    )
}

fn absorb_kv_impl(event: &mut Map, field: &str, options: Option<&Map>) -> AbsorbResult {
    let opts = match AbsorbOptions::from_map(options) {
        Ok(opts) => opts,
        Err(err) => return AbsorbResult::invalid_option(err),
    };

    let field_value = match event.get(field) {
        Some(value) => value.clone(),
        None => return AbsorbResult::new(AbsorbStatus::MissingField),
    };

    let immutable = match field_value.try_cast::<ImmutableString>() {
        Some(value) => value,
        None => return AbsorbResult::new(AbsorbStatus::NotString),
    };

    let text = immutable.into_owned();
    let mut tokens = opts.separator.split_tokens(&text);
    let had_tokens = !tokens.is_empty();
    let mut remainder_tokens: Vec<String> = Vec::new();
    let mut parsed_pairs: Vec<(String, String)> = Vec::new();

    for token in tokens.drain(..) {
        let token_str = token.as_str();
        if let Some(idx) = token_str.find(&opts.kv_sep) {
            let key = token_str[..idx].trim();
            let value = token_str[idx + opts.kv_sep.len()..].trim();

            if key.is_empty() {
                remainder_tokens.push(token);
                continue;
            }

            parsed_pairs.push((key.to_string(), value.to_string()));
        } else {
            remainder_tokens.push(token);
        }
    }

    let remainder = opts.separator.join_tokens(&remainder_tokens);
    let mut result = AbsorbResult::new(if parsed_pairs.is_empty() {
        AbsorbStatus::Empty
    } else {
        AbsorbStatus::Applied
    });
    result.remainder = remainder.clone();
    result.data = build_data_map(&parsed_pairs);

    if parsed_pairs.is_empty() {
        if !opts.keep_source && remainder.is_none() && !had_tokens && event.remove(field).is_some()
        {
            result.removed_source = true;
        }

        return result;
    }

    let mut wrote = false;
    let preexisting_keys = if opts.overwrite {
        None
    } else {
        Some(
            event
                .keys()
                .map(|key| key.to_string())
                .collect::<HashSet<String>>(),
        )
    };

    for (key, value) in &parsed_pairs {
        if !opts.overwrite {
            if let Some(existing) = &preexisting_keys {
                if existing.contains(key) {
                    continue;
                }
            }

            event.insert(key.clone().into(), Dynamic::from(value.clone()));
            wrote = true;
            continue;
        }

        event.insert(key.clone().into(), Dynamic::from(value.clone()));
        wrote = true;
    }

    result.written = wrote;

    if !opts.keep_source {
        match remainder {
            Some(ref text) => {
                event.insert(field.into(), Dynamic::from(text.clone()));
            }
            None => {
                if event.remove(field).is_some() {
                    result.removed_source = true;
                }
            }
        }
    }

    result
}

fn absorb_logfmt_impl(event: &mut Map, field: &str, options: Option<&Map>) -> AbsorbResult {
    let opts = match AbsorbOptions::from_map(options) {
        Ok(opts) => opts,
        Err(err) => return AbsorbResult::invalid_option(err),
    };

    let field_value = match event.get(field) {
        Some(value) => value.clone(),
        None => return AbsorbResult::new(AbsorbStatus::MissingField),
    };

    let immutable = match field_value.try_cast::<ImmutableString>() {
        Some(value) => value,
        None => return AbsorbResult::new(AbsorbStatus::NotString),
    };

    let text = immutable.into_owned();
    if text.trim().is_empty() {
        let mut result = AbsorbResult::new(AbsorbStatus::Empty);
        if !opts.keep_source && event.remove(field).is_some() {
            result.removed_source = true;
        }
        return result;
    }

    // Unlike absorb_kv's whitespace splitter, this delegates to the quote-aware,
    // type-inferring logfmt parser. It is all-or-nothing (like absorb_json): a
    // bare, unpaired token makes the whole field a parse_error rather than a
    // partial extraction, so there is no remainder.
    let data_map = match crate::rhai_functions::parsers::parse_logfmt_checked(&text) {
        Ok(map) => map,
        Err(err) => return AbsorbResult::parse_error(format!("invalid logfmt: {}", err)),
    };

    if data_map.is_empty() {
        let mut result = AbsorbResult::new(AbsorbStatus::Empty);
        if !opts.keep_source && event.remove(field).is_some() {
            result.removed_source = true;
        }
        return result;
    }

    let mut result = AbsorbResult::new(AbsorbStatus::Applied);
    result.data = data_map.clone();

    let preexisting_keys = if opts.overwrite {
        None
    } else {
        Some(
            event
                .keys()
                .map(|key| key.to_string())
                .collect::<HashSet<String>>(),
        )
    };

    for (key, value) in data_map.iter() {
        if !opts.overwrite {
            if let Some(existing) = &preexisting_keys {
                if existing.contains(key.as_str()) {
                    continue;
                }
            }
        }

        event.insert(key.clone(), value.clone());
        result.written = true;
    }

    if !opts.keep_source && event.remove(field).is_some() {
        result.removed_source = true;
    }

    result
}

fn absorb_jwt_impl(event: &mut Map, field: &str, options: Option<&Map>) -> AbsorbResult {
    let opts = match AbsorbOptions::from_map(options) {
        Ok(opts) => opts,
        Err(err) => return AbsorbResult::invalid_option(err),
    };

    let field_value = match event.get(field) {
        Some(value) => value.clone(),
        None => return AbsorbResult::new(AbsorbStatus::MissingField),
    };

    let immutable = match field_value.try_cast::<ImmutableString>() {
        Some(value) => value,
        None => return AbsorbResult::new(AbsorbStatus::NotString),
    };

    let text = immutable.into_owned();
    if text.trim().is_empty() {
        let mut result = AbsorbResult::new(AbsorbStatus::Empty);
        if !opts.keep_source && event.remove(field).is_some() {
            result.removed_source = true;
        }
        return result;
    }

    // Flatten only the JWT claims (the payload is a JSON object), mirroring
    // absorb_json. It is all-or-nothing: a malformed token is a parse_error and
    // the event is left untouched, never partially extracted. Signatures are
    // not verified — debugging / trusted tokens only. `sep`/`kv_sep` options are
    // accepted but ignored, since a JWT's structure is fixed.
    let data_map = match crate::rhai_functions::parsers::parse_jwt_claims_checked(&text) {
        Ok(map) => map,
        Err(err) => return AbsorbResult::parse_error(format!("invalid JWT: {}", err)),
    };

    if data_map.is_empty() {
        let mut result = AbsorbResult::new(AbsorbStatus::Empty);
        if !opts.keep_source && event.remove(field).is_some() {
            result.removed_source = true;
        }
        return result;
    }

    let mut result = AbsorbResult::new(AbsorbStatus::Applied);
    result.data = data_map.clone();

    let preexisting_keys = if opts.overwrite {
        None
    } else {
        Some(
            event
                .keys()
                .map(|key| key.to_string())
                .collect::<HashSet<String>>(),
        )
    };

    for (key, value) in data_map.iter() {
        if !opts.overwrite {
            if let Some(existing) = &preexisting_keys {
                if existing.contains(key.as_str()) {
                    continue;
                }
            }
        }

        event.insert(key.clone(), value.clone());
        result.written = true;
    }

    if !opts.keep_source && event.remove(field).is_some() {
        result.removed_source = true;
    }

    result
}

fn absorb_json_impl(event: &mut Map, field: &str, options: Option<&Map>) -> AbsorbResult {
    let opts = match AbsorbOptions::from_map(options) {
        Ok(opts) => opts,
        Err(err) => return AbsorbResult::invalid_option(err),
    };

    let field_value = match event.get(field) {
        Some(value) => value.clone(),
        None => return AbsorbResult::new(AbsorbStatus::MissingField),
    };

    let immutable = match field_value.try_cast::<ImmutableString>() {
        Some(value) => value,
        None => return AbsorbResult::new(AbsorbStatus::NotString),
    };

    let text = immutable.into_owned();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        let mut result = AbsorbResult::new(AbsorbStatus::Empty);
        if !opts.keep_source && event.remove(field).is_some() {
            result.removed_source = true;
        }
        return result;
    }

    let parsed = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => value,
        Err(err) => {
            return AbsorbResult::parse_error(format!("invalid JSON: {}", err));
        }
    };

    let object = match parsed {
        serde_json::Value::Object(obj) => obj,
        serde_json::Value::Array(_) => {
            return AbsorbResult::parse_error(
                "absorb_json expects a JSON object, got array".to_string(),
            );
        }
        serde_json::Value::String(_) => {
            return AbsorbResult::parse_error(
                "absorb_json expects a JSON object, got string".to_string(),
            );
        }
        serde_json::Value::Number(_) => {
            return AbsorbResult::parse_error(
                "absorb_json expects a JSON object, got number".to_string(),
            );
        }
        serde_json::Value::Bool(_) => {
            return AbsorbResult::parse_error(
                "absorb_json expects a JSON object, got bool".to_string(),
            );
        }
        serde_json::Value::Null => {
            return AbsorbResult::parse_error(
                "absorb_json expects a JSON object, got null".to_string(),
            );
        }
    };

    let mut data_map = Map::new();
    for (key, value) in object {
        data_map.insert(key.into(), json_to_dynamic(&value));
    }

    let mut result = AbsorbResult::new(AbsorbStatus::Applied);
    result.data = data_map.clone();

    let preexisting_keys = if opts.overwrite {
        None
    } else {
        Some(
            event
                .keys()
                .map(|key| key.to_string())
                .collect::<HashSet<String>>(),
        )
    };

    for (key, value) in data_map.iter() {
        if !opts.overwrite {
            if let Some(existing) = &preexisting_keys {
                if existing.contains(key.as_str()) {
                    continue;
                }
            }
        }

        event.insert(key.clone(), value.clone());
        result.written = true;
    }

    if !opts.keep_source && event.remove(field).is_some() {
        result.removed_source = true;
    }

    result
}

fn get_or_compile_regex(pattern: &str) -> Result<Regex, String> {
    // Check cache first
    if let Some(regex) = ABSORB_REGEX_CACHE.with(|cache| cache.borrow_mut().get(pattern).cloned()) {
        return Ok(regex);
    }

    // Compile new regex
    let regex = Regex::new(pattern).map_err(|err| format!("Invalid regex pattern: {}", err))?;

    // Store in cache
    ABSORB_REGEX_CACHE.with(|cache| {
        cache.borrow_mut().put(pattern.to_string(), regex.clone());
    });

    Ok(regex)
}

fn absorb_regex_impl(
    event: &mut Map,
    field: &str,
    pattern: &str,
    options: Option<&Map>,
) -> AbsorbResult {
    let opts = match AbsorbOptions::from_map(options) {
        Ok(opts) => opts,
        Err(err) => return AbsorbResult::invalid_option(err),
    };

    let field_value = match event.get(field) {
        Some(value) => value.clone(),
        None => return AbsorbResult::new(AbsorbStatus::MissingField),
    };

    let immutable = match field_value.try_cast::<ImmutableString>() {
        Some(value) => value,
        None => return AbsorbResult::new(AbsorbStatus::NotString),
    };

    let text = immutable.into_owned();

    // Get or compile the regex (uses cache)
    let re = match get_or_compile_regex(pattern) {
        Ok(re) => re,
        Err(err) => {
            return AbsorbResult::parse_error(err);
        }
    };

    // Try to match the pattern
    let caps = match re.captures(&text) {
        Some(caps) => caps,
        None => {
            let mut result = AbsorbResult::new(AbsorbStatus::Empty);
            if !opts.keep_source && event.remove(field).is_some() {
                result.removed_source = true;
            }
            return result;
        }
    };

    // Extract named capture groups
    let mut data_map = Map::new();
    let mut extracted_fields = Vec::new();

    for name in re.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            let value = m.as_str().to_string();
            data_map.insert(name.into(), Dynamic::from(value.clone()));
            extracted_fields.push((name.to_string(), value));
        }
    }

    if extracted_fields.is_empty() {
        let mut result = AbsorbResult::new(AbsorbStatus::Empty);
        if !opts.keep_source && event.remove(field).is_some() {
            result.removed_source = true;
        }
        return result;
    }

    let mut result = AbsorbResult::new(AbsorbStatus::Applied);
    result.data = data_map.clone();

    let preexisting_keys = if opts.overwrite {
        None
    } else {
        Some(
            event
                .keys()
                .map(|key| key.to_string())
                .collect::<HashSet<String>>(),
        )
    };

    for (key, value) in data_map.iter() {
        if !opts.overwrite {
            if let Some(existing) = &preexisting_keys {
                if existing.contains(key.as_str()) {
                    continue;
                }
            }
        }

        event.insert(key.clone(), value.clone());
        result.written = true;
    }

    if !opts.keep_source && event.remove(field).is_some() {
        result.removed_source = true;
    }

    result
}

fn build_data_map(pairs: &[(String, String)]) -> Map {
    let mut data = Map::new();
    for (key, value) in pairs {
        data.insert(key.clone().into(), Dynamic::from(value.clone()));
    }
    data
}

#[derive(Debug, Clone)]
struct AbsorbResult {
    status: AbsorbStatus,
    data: Map,
    written: bool,
    remainder: Option<String>,
    removed_source: bool,
    error: Option<String>,
}

impl AbsorbResult {
    fn new(status: AbsorbStatus) -> Self {
        Self {
            status,
            data: Map::new(),
            written: false,
            remainder: None,
            removed_source: false,
            error: None,
        }
    }

    fn invalid_option(err: OptionsError) -> Self {
        Self {
            status: AbsorbStatus::InvalidOption,
            data: Map::new(),
            written: false,
            remainder: None,
            removed_source: false,
            error: Some(err.message),
        }
    }

    fn parse_error(message: String) -> Self {
        Self {
            status: AbsorbStatus::ParseError,
            data: Map::new(),
            written: false,
            remainder: None,
            removed_source: false,
            error: Some(message),
        }
    }

    fn into_map(self) -> Map {
        let mut map = Map::new();
        map.insert("status".into(), Dynamic::from(self.status.as_str()));
        map.insert("data".into(), Dynamic::from(self.data));
        map.insert("written".into(), Dynamic::from(self.written));
        match self.remainder {
            Some(text) => {
                map.insert("remainder".into(), Dynamic::from(text));
            }
            None => {
                map.insert("remainder".into(), Dynamic::UNIT);
            }
        }
        map.insert("removed_source".into(), Dynamic::from(self.removed_source));
        match self.error {
            Some(err) => {
                map.insert("error".into(), Dynamic::from(err));
            }
            None => {
                map.insert("error".into(), Dynamic::UNIT);
            }
        }
        map
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AbsorbStatus {
    Applied,
    MissingField,
    NotString,
    Empty,
    ParseError,
    InvalidOption,
}

impl AbsorbStatus {
    fn as_str(&self) -> &'static str {
        match self {
            AbsorbStatus::Applied => "applied",
            AbsorbStatus::MissingField => "missing_field",
            AbsorbStatus::NotString => "not_string",
            AbsorbStatus::Empty => "empty",
            AbsorbStatus::ParseError => "parse_error",
            AbsorbStatus::InvalidOption => "invalid_option",
        }
    }
}

#[derive(Debug, Clone)]
struct AbsorbOptions {
    separator: TokenSeparator,
    kv_sep: String,
    keep_source: bool,
    overwrite: bool,
}

impl Default for AbsorbOptions {
    fn default() -> Self {
        Self {
            separator: TokenSeparator::Whitespace,
            kv_sep: "=".to_string(),
            keep_source: false,
            overwrite: true,
        }
    }
}

impl AbsorbOptions {
    fn from_map(map: Option<&Map>) -> Result<Self, OptionsError> {
        let mut options = Self::default();

        if let Some(opts) = map {
            for (key, value) in opts.iter() {
                match key.as_str() {
                    "sep" => {
                        if value.is_unit() {
                            options.separator = TokenSeparator::Whitespace;
                        } else if let Some(sep) = value.clone().try_cast::<ImmutableString>() {
                            let sep = sep.into_owned();
                            if sep.is_empty() {
                                return Err(OptionsError::invalid_value(
                                    "sep",
                                    "must not be empty",
                                ));
                            }
                            options.separator = TokenSeparator::Literal(sep);
                        } else {
                            return Err(OptionsError::invalid_type("sep", "string or ()"));
                        }
                    }
                    "kv_sep" => {
                        if let Some(sep) = value.clone().try_cast::<ImmutableString>() {
                            let sep = sep.into_owned();
                            if sep.is_empty() {
                                return Err(OptionsError::invalid_value(
                                    "kv_sep",
                                    "must not be empty",
                                ));
                            }
                            options.kv_sep = sep;
                        } else {
                            return Err(OptionsError::invalid_type("kv_sep", "string"));
                        }
                    }
                    "keep_source" => {
                        if let Some(flag) = value.clone().try_cast::<bool>() {
                            options.keep_source = flag;
                        } else {
                            return Err(OptionsError::invalid_type("keep_source", "bool"));
                        }
                    }
                    "overwrite" => {
                        if let Some(flag) = value.clone().try_cast::<bool>() {
                            options.overwrite = flag;
                        } else {
                            return Err(OptionsError::invalid_type("overwrite", "bool"));
                        }
                    }
                    other => {
                        return Err(OptionsError::unknown(other));
                    }
                }
            }
        }

        Ok(options)
    }
}

#[derive(Debug, Clone)]
enum TokenSeparator {
    Whitespace,
    Literal(String),
}

impl TokenSeparator {
    fn split_tokens(&self, text: &str) -> Vec<String> {
        match self {
            TokenSeparator::Whitespace => text
                .split_whitespace()
                .map(|token| token.to_string())
                .collect(),
            TokenSeparator::Literal(sep) => text
                .split(sep)
                .map(|token| token.trim())
                .filter(|token| !token.is_empty())
                .map(|token| token.to_string())
                .collect(),
        }
    }

    fn join_tokens(&self, tokens: &[String]) -> Option<String> {
        if tokens.is_empty() {
            return None;
        }

        let joined = match self {
            TokenSeparator::Whitespace => tokens.join(" "),
            TokenSeparator::Literal(sep) => tokens.join(sep),
        };

        Some(joined)
    }
}

#[derive(Debug, Clone)]
struct OptionsError {
    message: String,
}

impl OptionsError {
    fn unknown(key: &str) -> Self {
        Self {
            message: format!("unknown absorb option: {}", key),
        }
    }

    fn invalid_type(key: &str, expected: &str) -> Self {
        Self {
            message: format!(
                "invalid absorb option type for {}: expected {}",
                key, expected
            ),
        }
    }

    fn invalid_value(key: &str, message: &str) -> Self {
        Self {
            message: format!("invalid value for absorb option {}: {}", key, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_string(value: &str) -> Dynamic {
        Dynamic::from(value.to_string())
    }

    #[test]
    fn absorb_kv_basic_merge() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert(
            "msg".into(),
            map_string("Payment timeout order=1234 gateway=stripe"),
        );

        let result = absorb_kv_impl(&mut event, "msg", None);
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(result.written);
        assert_eq!(result.remainder.as_deref(), Some("Payment timeout"));
        assert_eq!(event.get("order").unwrap().to_string(), "1234");
        assert_eq!(event.get("gateway").unwrap().to_string(), "stripe");
    }

    #[test]
    fn absorb_kv_keep_source_preserves_field() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("prefix user=alice suffix"));

        let mut options = Map::new();
        options.insert("keep_source".into(), Dynamic::from(true));

        let result = absorb_kv_impl(&mut event, "msg", Some(&options));
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert_eq!(
            event.get("msg").unwrap().to_string(),
            "prefix user=alice suffix"
        );
        assert_eq!(result.remainder.as_deref(), Some("prefix suffix"));
    }

    #[test]
    fn absorb_kv_overwrite_false_skips_existing() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("status".into(), map_string("pending"));
        event.insert("msg".into(), map_string("Processing status=active"));

        let mut options = Map::new();
        options.insert("overwrite".into(), Dynamic::from(false));

        let result = absorb_kv_impl(&mut event, "msg", Some(&options));
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(!result.written);
        assert_eq!(event.get("status").unwrap().to_string(), "pending");
        assert_eq!(result.data.get("status").unwrap().to_string(), "active");
    }

    #[test]
    fn absorb_kv_invalid_option_sets_status() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("user=alice"));

        let mut options = Map::new();
        options.insert("keep_sorce".into(), Dynamic::from(true));

        let result = absorb_kv_impl(&mut event, "msg", Some(&options));
        assert_eq!(result.status, AbsorbStatus::InvalidOption);
        assert_eq!(
            result.error.as_deref(),
            Some("unknown absorb option: keep_sorce")
        );
    }

    #[test]
    fn absorb_kv_empty_string_removes_field() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("   "));

        let result = absorb_kv_impl(&mut event, "msg", None);
        assert_eq!(result.status, AbsorbStatus::Empty);
        assert!(result.remainder.is_none());
        assert!(!event.contains_key("msg"));
        assert!(result.removed_source);
    }

    #[test]
    fn absorb_logfmt_quote_aware_typed_merge() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert(
            "msg".into(),
            map_string(r#"pod="kube-system/foo" err="connection refused" replicas=3"#),
        );

        let result = absorb_logfmt_impl(&mut event, "msg", None);
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(result.written);
        assert!(result.removed_source);
        // Surrounding quotes stripped, embedded spaces preserved (unlike absorb_kv).
        assert_eq!(event.get("pod").unwrap().to_string(), "kube-system/foo");
        assert_eq!(event.get("err").unwrap().to_string(), "connection refused");
        // Numeric values are typed, like absorb_json.
        assert_eq!(event.get("replicas").unwrap().as_int().unwrap(), 3);
    }

    #[test]
    fn absorb_logfmt_keep_source_and_overwrite_false() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("user".into(), map_string("existing"));
        event.insert("msg".into(), map_string(r#"user=alice role=admin"#));

        let mut options = Map::new();
        options.insert("keep_source".into(), Dynamic::from(true));
        options.insert("overwrite".into(), Dynamic::from(false));

        let result = absorb_logfmt_impl(&mut event, "msg", Some(&options));
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(!result.removed_source);
        assert_eq!(
            event.get("msg").unwrap().to_string(),
            "user=alice role=admin"
        );
        assert_eq!(event.get("user").unwrap().to_string(), "existing"); // skipped conflict
        assert_eq!(event.get("role").unwrap().to_string(), "admin");
        assert_eq!(result.data.get("user").unwrap().to_string(), "alice");
    }

    #[test]
    fn absorb_logfmt_bare_token_is_parse_error() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("prefix user=alice suffix"));

        // logfmt is all-or-nothing: a bare token makes the whole field invalid,
        // and the event is left untouched (mirroring absorb_json).
        let result = absorb_logfmt_impl(&mut event, "msg", None);
        assert_eq!(result.status, AbsorbStatus::ParseError);
        assert!(result.error.as_ref().unwrap().contains("invalid logfmt"));
        assert_eq!(
            event.get("msg").unwrap().to_string(),
            "prefix user=alice suffix"
        );
        assert!(!event.contains_key("user"));
    }

    #[test]
    fn absorb_logfmt_empty_string_removes_field() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("   "));

        let result = absorb_logfmt_impl(&mut event, "msg", None);
        assert_eq!(result.status, AbsorbStatus::Empty);
        assert!(result.removed_source);
        assert!(!event.contains_key("msg"));
    }

    #[test]
    fn absorb_logfmt_missing_field() {
        set_absorb_strict(false);
        let mut event = Map::new();

        let result = absorb_logfmt_impl(&mut event, "msg", None);
        assert_eq!(result.status, AbsorbStatus::MissingField);
    }

    // Header {"alg":"none"}, payload {"sub":"alice","role":"admin","exp":2000000000}
    // (unsigned, no signature segment).
    const JWT_ALICE: &str = "eyJhbGciOiJub25lIn0.\
        eyJzdWIiOiJhbGljZSIsInJvbGUiOiJhZG1pbiIsImV4cCI6MjAwMDAwMDAwMH0";

    #[test]
    fn absorb_jwt_merges_claims() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("token".into(), map_string(JWT_ALICE));

        let result = absorb_jwt_impl(&mut event, "token", None);
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(result.written);
        assert!(result.removed_source);
        assert!(!event.contains_key("token"));
        // Claims are flattened to top level; the header is ignored.
        assert_eq!(event.get("sub").unwrap().to_string(), "alice");
        assert_eq!(event.get("role").unwrap().to_string(), "admin");
        assert_eq!(event.get("exp").unwrap().as_int().unwrap(), 2000000000);
        assert!(!event.contains_key("alg"));
        assert_eq!(result.data.get("sub").unwrap().to_string(), "alice");
    }

    #[test]
    fn absorb_jwt_keep_source_and_overwrite_false() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("role".into(), map_string("existing"));
        event.insert("token".into(), map_string(JWT_ALICE));

        let mut options = Map::new();
        options.insert("keep_source".into(), Dynamic::from(true));
        options.insert("overwrite".into(), Dynamic::from(false));

        let result = absorb_jwt_impl(&mut event, "token", Some(&options));
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(!result.removed_source);
        assert_eq!(event.get("token").unwrap().to_string(), JWT_ALICE);
        assert_eq!(event.get("role").unwrap().to_string(), "existing"); // conflict skipped
        assert_eq!(event.get("sub").unwrap().to_string(), "alice");
        assert_eq!(result.data.get("role").unwrap().to_string(), "admin");
    }

    #[test]
    fn absorb_jwt_malformed_token_is_parse_error() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("token".into(), map_string("not-a-jwt"));

        let result = absorb_jwt_impl(&mut event, "token", None);
        assert_eq!(result.status, AbsorbStatus::ParseError);
        assert!(result.error.as_ref().unwrap().contains("invalid JWT"));
        // All-or-nothing: the event is left untouched.
        assert_eq!(event.get("token").unwrap().to_string(), "not-a-jwt");
    }

    #[test]
    fn absorb_jwt_empty_string_removes_field() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("token".into(), map_string("   "));

        let result = absorb_jwt_impl(&mut event, "token", None);
        assert_eq!(result.status, AbsorbStatus::Empty);
        assert!(result.removed_source);
        assert!(!event.contains_key("token"));
    }

    #[test]
    fn absorb_jwt_missing_field() {
        set_absorb_strict(false);
        let mut event = Map::new();

        let result = absorb_jwt_impl(&mut event, "token", None);
        assert_eq!(result.status, AbsorbStatus::MissingField);
    }

    #[test]
    fn absorb_json_basic_merge() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert(
            "payload".into(),
            map_string(r#"{ "user":"alice", "count": 42 }"#),
        );

        let result = absorb_json_impl(&mut event, "payload", None);
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(result.remainder.is_none());
        assert!(result.removed_source);
        assert_eq!(event.get("user").unwrap().to_string(), "alice");
        assert_eq!(event.get("count").unwrap().as_int().unwrap(), 42);
        assert_eq!(result.data.get("user").unwrap().to_string(), "alice");
    }

    #[test]
    fn absorb_json_keep_source_preserves_field() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert(
            "payload".into(),
            map_string(r#"  { "status": "ok", "detail": { "code": 200 } }  "#),
        );

        let mut options = Map::new();
        options.insert("keep_source".into(), Dynamic::from(true));

        let result = absorb_json_impl(&mut event, "payload", Some(&options));
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert_eq!(
            event.get("payload").unwrap().to_string(),
            r#"  { "status": "ok", "detail": { "code": 200 } }  "#
        );
        let detail = event.get("detail").unwrap().clone().cast::<Map>();
        assert_eq!(detail.get("code").unwrap().clone().cast::<i64>(), 200);
        assert!(!result.removed_source);
    }

    #[test]
    fn absorb_json_overwrite_false_skips_existing() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("user".into(), map_string("existing"));
        event.insert(
            "payload".into(),
            map_string(r#"{ "user": "alice", "role": "admin" }"#),
        );

        let mut options = Map::new();
        options.insert("overwrite".into(), Dynamic::from(false));

        let result = absorb_json_impl(&mut event, "payload", Some(&options));
        assert_eq!(result.status, AbsorbStatus::Applied);
        assert_eq!(event.get("user").unwrap().to_string(), "existing");
        assert_eq!(event.get("role").unwrap().to_string(), "admin");
        assert!(result.written); // role inserted
    }

    #[test]
    fn absorb_json_invalid_json_sets_parse_error() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("payload".into(), map_string(r#"{ "user": "alice""#));

        let result = absorb_json_impl(&mut event, "payload", None);
        assert_eq!(result.status, AbsorbStatus::ParseError);
        assert!(result.error.as_ref().unwrap().contains("invalid JSON"));
        assert!(event.contains_key("payload"));
    }

    #[test]
    fn absorb_json_non_object_returns_error() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("payload".into(), map_string(r#"[1, 2, 3]"#));

        let result = absorb_json_impl(&mut event, "payload", None);
        assert_eq!(result.status, AbsorbStatus::ParseError);
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("expects a JSON object"));
    }

    #[test]
    fn absorb_json_whitespace_only_removes_field() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("payload".into(), map_string("   "));

        let result = absorb_json_impl(&mut event, "payload", None);
        assert_eq!(result.status, AbsorbStatus::Empty);
        assert!(result.removed_source);
        assert!(!event.contains_key("payload"));
    }

    #[test]
    fn absorb_regex_basic_extraction() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert(
            "msg".into(),
            map_string("User alice logged in from 192.168.1.1"),
        );

        let pattern = r"User (?P<user>\w+) logged in from (?P<ip>[\d.]+)";
        let result = absorb_regex_impl(&mut event, "msg", pattern, None);

        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(result.removed_source);
        assert_eq!(event.get("user").unwrap().to_string(), "alice");
        assert_eq!(event.get("ip").unwrap().to_string(), "192.168.1.1");
        assert_eq!(result.data.get("user").unwrap().to_string(), "alice");
    }

    #[test]
    fn absorb_regex_keep_source() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("Status: 200 Duration: 45ms"));

        let mut options = Map::new();
        options.insert("keep_source".into(), Dynamic::from(true));

        let pattern = r"Status: (?P<status>\d+) Duration: (?P<duration>\d+)ms";
        let result = absorb_regex_impl(&mut event, "msg", pattern, Some(&options));

        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(!result.removed_source);
        assert_eq!(
            event.get("msg").unwrap().to_string(),
            "Status: 200 Duration: 45ms"
        );
        assert_eq!(event.get("status").unwrap().to_string(), "200");
        assert_eq!(event.get("duration").unwrap().to_string(), "45");
    }

    #[test]
    fn absorb_regex_overwrite_false() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("status".into(), map_string("existing"));
        event.insert("msg".into(), map_string("Status: 200"));

        let mut options = Map::new();
        options.insert("overwrite".into(), Dynamic::from(false));

        let pattern = r"Status: (?P<status>\d+)";
        let result = absorb_regex_impl(&mut event, "msg", pattern, Some(&options));

        assert_eq!(result.status, AbsorbStatus::Applied);
        assert!(!result.written);
        assert_eq!(event.get("status").unwrap().to_string(), "existing");
        assert_eq!(result.data.get("status").unwrap().to_string(), "200");
    }

    #[test]
    fn absorb_regex_no_match() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("No pattern here"));

        let pattern = r"User (?P<user>\w+)";
        let result = absorb_regex_impl(&mut event, "msg", pattern, None);

        assert_eq!(result.status, AbsorbStatus::Empty);
        assert!(result.removed_source);
        assert!(!event.contains_key("msg"));
    }

    #[test]
    fn absorb_regex_invalid_pattern() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), map_string("Some text"));

        let pattern = r"Invalid (?P<unclosed";
        let result = absorb_regex_impl(&mut event, "msg", pattern, None);

        assert_eq!(result.status, AbsorbStatus::ParseError);
        assert!(result.error.as_ref().unwrap().contains("Invalid regex"));
        assert!(event.contains_key("msg"));
    }

    #[test]
    fn absorb_regex_missing_field() {
        set_absorb_strict(false);
        let mut event = Map::new();

        let pattern = r"User (?P<user>\w+)";
        let result = absorb_regex_impl(&mut event, "msg", pattern, None);

        assert_eq!(result.status, AbsorbStatus::MissingField);
    }

    #[test]
    fn absorb_regex_not_string() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert("msg".into(), Dynamic::from(123));

        let pattern = r"User (?P<user>\w+)";
        let result = absorb_regex_impl(&mut event, "msg", pattern, None);

        assert_eq!(result.status, AbsorbStatus::NotString);
    }

    #[test]
    fn absorb_regex_multiple_captures() {
        set_absorb_strict(false);
        let mut event = Map::new();
        event.insert(
            "msg".into(),
            map_string("2024-01-15 ERROR main.rs:42 Database connection failed"),
        );

        let pattern =
            r"(?P<date>[\d-]+) (?P<level>\w+) (?P<file>[\w.]+):(?P<line>\d+) (?P<message>.+)";
        let result = absorb_regex_impl(&mut event, "msg", pattern, None);

        assert_eq!(result.status, AbsorbStatus::Applied);
        assert_eq!(event.get("date").unwrap().to_string(), "2024-01-15");
        assert_eq!(event.get("level").unwrap().to_string(), "ERROR");
        assert_eq!(event.get("file").unwrap().to_string(), "main.rs");
        assert_eq!(event.get("line").unwrap().to_string(), "42");
        assert_eq!(
            event.get("message").unwrap().to_string(),
            "Database connection failed"
        );
    }
}
