use super::*;
use rhai::{Map, Scope};

/// Helper to register string, parser, serializer, and extractor functions for tests
fn register_all_string_functions(engine: &mut rhai::Engine) {
    register_functions(engine);
    crate::rhai_functions::parsers::register_functions(engine);
    crate::rhai_functions::emit::register_functions(engine);
    crate::rhai_functions::maps::register_functions(engine);
    crate::rhai_functions::serializers::register_functions(engine);
    crate::rhai_functions::extractors::register_functions(engine);
}

#[test]
fn test_after_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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

// Note: test_parse_jwt_function moved to jwt.rs

#[test]
fn test_parse_syslog_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
fn test_extract_regex_maps_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

    let mut scope = Scope::new();
    scope.push("text", "alice@test.com and bob@example.org");

    // Extract emails without capture groups (uses full match)
    let result: rhai::Array = engine
        .eval_with_scope(
            &mut scope,
            r##"text.extract_regex_maps("\\w+@\\w+\\.\\w+", "email")"##,
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
            r##"usertext.extract_regex_maps("user=(\\w+)", "username")"##,
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
            r##"nomatch.extract_regex_maps("\\w+@\\w+", "email")"##,
        )
        .unwrap();
    assert_eq!(result.len(), 0);

    // Invalid regex (returns empty array)
    let result: rhai::Array = engine
        .eval_with_scope(&mut scope, r##"text.extract_regex_maps("[", "invalid")"##)
        .unwrap();
    assert_eq!(result.len(), 0);
}

#[test]
fn test_extract_regex_maps_with_emit_each() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);
    register_all_string_functions(&mut engine);

    let mut scope = Scope::new();
    scope.push("text", "Found IPs: 192.168.1.1 and 10.0.0.1");

    // Test composability with emit_each
    let result: i64 = engine
        .eval_with_scope(
            &mut scope,
            r##"
            let ip_maps = text.extract_regex_maps("\\b(?:\\d{1,3}\\.){3}\\d{1,3}\\b", "ip");
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
fn test_split_regex_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

    let mut scope = Scope::new();
    scope.push("text", "one,two;three:four");

    // Split by multiple delimiters
    let result: rhai::Array = engine
        .eval_with_scope(&mut scope, r##"text.split_regex("[,;:]")"##)
        .unwrap();
    assert_eq!(result.len(), 4);
    assert_eq!(result[0].clone().into_string().unwrap(), "one");
    assert_eq!(result[1].clone().into_string().unwrap(), "two");
    assert_eq!(result[2].clone().into_string().unwrap(), "three");
    assert_eq!(result[3].clone().into_string().unwrap(), "four");

    // Split by whitespace
    scope.push("spaced", "hello    world\ttab\nnewline");
    let result: rhai::Array = engine
        .eval_with_scope(&mut scope, r##"spaced.split_regex("\\s+")"##)
        .unwrap();
    assert_eq!(result.len(), 4);
    assert_eq!(result[0].clone().into_string().unwrap(), "hello");
    assert_eq!(result[1].clone().into_string().unwrap(), "world");

    // Invalid regex (returns original string)
    let result: rhai::Array = engine
        .eval_with_scope(&mut scope, r##"text.split_regex("[")"##)
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].clone().into_string().unwrap(),
        "one,two;three:four"
    );
}

#[test]
fn test_replace_regex_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

    let mut scope = Scope::new();
    scope.push("text", "The year 2023 and 2024 are here");

    // Replace all years with "YEAR"
    let result: String = engine
        .eval_with_scope(&mut scope, r##"text.replace_regex("\\d{4}", "YEAR")"##)
        .unwrap();
    assert_eq!(result, "The year YEAR and YEAR are here");

    // Replace with capture groups
    scope.push("emails", "Contact alice@example.com or bob@test.org");
    let result: String = engine
        .eval_with_scope(
            &mut scope,
            r##"emails.replace_regex("(\\w+)@(\\w+\\.\\w+)", "[$1 at $2]")"##,
        )
        .unwrap();
    assert_eq!(
        result,
        "Contact [alice at example.com] or [bob at test.org]"
    );

    // No matches (returns original)
    let result: String = engine
        .eval_with_scope(
            &mut scope,
            r##"text.replace_regex("nomatch", "replacement")"##,
        )
        .unwrap();
    assert_eq!(result, "The year 2023 and 2024 are here");

    // Invalid regex (returns original)
    let result: String = engine
        .eval_with_scope(&mut scope, r##"text.replace_regex("[", "replacement")"##)
        .unwrap();
    assert_eq!(result, "The year 2023 and 2024 are here");
}

#[test]
fn test_extract_ip_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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

// Note: test_mask_ip_function and test_is_private_ip_function moved to ip_utils.rs

#[test]
fn test_extract_url_function() {
    let mut engine = rhai::Engine::new();
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
    register_all_string_functions(&mut engine);

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
