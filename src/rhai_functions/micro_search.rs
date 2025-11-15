//! Micro search helpers for Rhai filters (`like`, `ilike`, `matches`)
use lru::LruCache;
use regex::{Regex, RegexBuilder};
use rhai::{Engine, EvalAltResult, Position};
use std::{cell::RefCell, num::NonZeroUsize};
use unicode_normalization::UnicodeNormalization;

const REGEX_CACHE_CAPACITY: usize = 1000;
const REGEX_SIZE_LIMIT_BYTES: usize = 1 << 20; // 1 MiB compile budget
const REGEX_DFA_SIZE_LIMIT_BYTES: usize = 1 << 20; // 1 MiB DFA cache budget

thread_local! {
    static REGEX_CACHE: RefCell<LruCache<String, Regex>> = RefCell::new(LruCache::new(
        NonZeroUsize::new(REGEX_CACHE_CAPACITY).expect("regex cache capacity must be non-zero")
    ));
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("like", |text: &str, pattern: &str| like_impl(text, pattern));
    engine.register_fn("ilike", |text: &str, pattern: &str| {
        ilike_impl(text, pattern)
    });
    engine.register_fn(
        "matches",
        |text: &str, pattern: &str| -> Result<bool, Box<EvalAltResult>> {
            matches_impl(text, pattern)
        },
    );
}

#[doc(hidden)]
pub fn like_impl(haystack: &str, pattern: &str) -> bool {
    if haystack.is_ascii() && pattern.is_ascii() {
        return glob_like_ascii(haystack.as_bytes(), pattern.as_bytes());
    }

    let haystack_chars: Vec<char> = haystack.chars().collect();
    let pattern_chars: Vec<char> = pattern.chars().collect();
    glob_like_unicode(&haystack_chars, &pattern_chars)
}

#[doc(hidden)]
pub fn ilike_impl(haystack: &str, pattern: &str) -> bool {
    if haystack.is_ascii() && pattern.is_ascii() {
        let hay = haystack.to_ascii_lowercase();
        let pat = pattern.to_ascii_lowercase();
        return glob_like_ascii(hay.as_bytes(), pat.as_bytes());
    }

    let haystack_norm = normalize_for_ilike(haystack);
    let pattern_norm = normalize_for_ilike(pattern);

    if haystack_norm.is_ascii() && pattern_norm.is_ascii() {
        return glob_like_ascii(haystack_norm.as_bytes(), pattern_norm.as_bytes());
    }

    let haystack_chars: Vec<char> = haystack_norm.chars().collect();
    let pattern_chars: Vec<char> = pattern_norm.chars().collect();
    glob_like_unicode(&haystack_chars, &pattern_chars)
}

#[doc(hidden)]
pub fn matches_impl(text: &str, pattern: &str) -> Result<bool, Box<EvalAltResult>> {
    let regex = get_or_compile_regex(pattern)?;
    Ok(regex.is_match(text))
}

fn get_or_compile_regex(pattern: &str) -> Result<Regex, Box<EvalAltResult>> {
    if let Some(regex) = REGEX_CACHE.with(|cache| cache.borrow_mut().get(pattern).cloned()) {
        return Ok(regex);
    }

    let regex = build_regex(pattern).map_err(|err| {
        Box::new(EvalAltResult::ErrorRuntime(
            format!("Invalid regex pattern '{}': {err}", pattern).into(),
            Position::NONE,
        ))
    })?;

    REGEX_CACHE.with(|cache| {
        cache.borrow_mut().put(pattern.to_string(), regex.clone());
    });

    Ok(regex)
}

fn build_regex(pattern: &str) -> Result<Regex, regex::Error> {
    let mut builder = RegexBuilder::new(pattern);
    builder.size_limit(REGEX_SIZE_LIMIT_BYTES);
    builder.dfa_size_limit(REGEX_DFA_SIZE_LIMIT_BYTES);
    builder.build()
}

fn glob_like_ascii(haystack: &[u8], pattern: &[u8]) -> bool {
    let mut h_idx = 0;
    let mut p_idx = 0;
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0;

    while h_idx < haystack.len() {
        if p_idx < pattern.len() && (pattern[p_idx] == b'?' || pattern[p_idx] == haystack[h_idx]) {
            h_idx += 1;
            p_idx += 1;
        } else if p_idx < pattern.len() && pattern[p_idx] == b'*' {
            star_idx = Some(p_idx);
            p_idx += 1;
            match_idx = h_idx;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            h_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < pattern.len() && pattern[p_idx] == b'*' {
        p_idx += 1;
    }
    p_idx == pattern.len()
}

fn glob_like_unicode(haystack: &[char], pattern: &[char]) -> bool {
    let mut h_idx = 0;
    let mut p_idx = 0;
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0;

    while h_idx < haystack.len() {
        if p_idx < pattern.len() && (pattern[p_idx] == '?' || pattern[p_idx] == haystack[h_idx]) {
            h_idx += 1;
            p_idx += 1;
        } else if p_idx < pattern.len() && pattern[p_idx] == '*' {
            star_idx = Some(p_idx);
            p_idx += 1;
            match_idx = h_idx;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            h_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < pattern.len() && pattern[p_idx] == '*' {
        p_idx += 1;
    }
    p_idx == pattern.len()
}

fn normalize_for_ilike(input: &str) -> String {
    let normalized: String = input.nfkc().collect();
    normalized.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::{Dynamic, Map as RhaiMap};

    #[test]
    fn like_ascii_positive_and_negative() {
        assert!(like_impl("access.log", "access*"));
        assert!(like_impl("foo", "f?o"));
        assert!(!like_impl("foo", "bar*"));
        assert!(!like_impl("foo", "f?oo"));
    }

    #[test]
    fn like_unicode_scalars() {
        assert!(like_impl("🚀launch", "🚀*"));
        assert!(like_impl("café", "caf?"));
        assert!(!like_impl("naïve", "nai?e"));
    }

    #[test]
    fn ilike_unicode_case_folding() {
        assert!(ilike_impl("Straße", "strasse"));
        assert!(ilike_impl("CAFÉ", "café"));
        assert!(ilike_impl("straße", "STRASSE"));
        assert!(!ilike_impl("straße", "street"));
    }

    #[test]
    fn like_empty_string_cases() {
        assert!(like_impl("", ""));
        assert!(like_impl("", "*"));
        assert!(like_impl("", "**"));
        assert!(!like_impl("", "?"));
        assert!(!like_impl("foo", ""));
    }

    #[test]
    fn has_checks_unit_semantics() {
        // Test that map.has() correctly handles unit values
        // Note: has() is registered in maps.rs module
        let mut engine = rhai::Engine::new();
        crate::rhai_functions::maps::register_functions(&mut engine);

        let mut map = RhaiMap::new();
        map.insert("present".into(), Dynamic::from("value"));
        map.insert("empty_string".into(), Dynamic::from(""));
        map.insert("unit".into(), Dynamic::UNIT);

        let mut scope = rhai::Scope::new();
        scope.push("map", map);

        assert!(engine
            .eval_with_scope::<bool>(&mut scope, "map.has(\"present\")")
            .unwrap());
        assert!(engine
            .eval_with_scope::<bool>(&mut scope, "map.has(\"empty_string\")")
            .unwrap());
        assert!(!engine
            .eval_with_scope::<bool>(&mut scope, "map.has(\"unit\")")
            .unwrap());
        assert!(!engine
            .eval_with_scope::<bool>(&mut scope, "map.has(\"missing\")")
            .unwrap());
    }

    #[test]
    fn matches_reports_errors_for_invalid_patterns() {
        let err = matches_impl("foo", "(").expect_err("pattern should be invalid");
        assert!(err.to_string().contains("Invalid regex pattern"));
    }

    #[test]
    fn matches_uses_cached_regex_between_calls() {
        assert!(matches_impl("user not found", r"user\s+not\s+found").unwrap());
        assert!(matches_impl("user not found", r"user\s+not\s+found").unwrap());
    }
}
