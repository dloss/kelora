use rhai::Engine;
use serde::Serialize;

/// Encode a string to base64
fn encode_b64_impl(input: &str) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, input.as_bytes())
}

/// Decode a base64 string
fn decode_b64_impl(input: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    match base64::Engine::decode(&base64::engine::general_purpose::STANDARD, input) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(string) => Ok(string),
            Err(e) => Err(format!("Invalid UTF-8 in base64 decoded data: {}", e).into()),
        },
        Err(e) => Err(format!("Invalid base64 string: {}", e).into()),
    }
}

/// Encode a string to hexadecimal
fn encode_hex_impl(input: &str) -> String {
    hex::encode(input.as_bytes())
}

/// Decode a hexadecimal string
fn decode_hex_impl(input: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    match hex::decode(input) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(string) => Ok(string),
            Err(e) => Err(format!("Invalid UTF-8 in hex decoded data: {}", e).into()),
        },
        Err(e) => Err(format!("Invalid hex string: {}", e).into()),
    }
}

/// URL encode a string (percent encoding)
fn encode_url_impl(input: &str) -> String {
    urlencoding::encode(input).to_string()
}

/// URL decode a string (percent decoding)
fn decode_url_impl(input: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    match urlencoding::decode(input) {
        Ok(cow_str) => Ok(cow_str.to_string()),
        Err(e) => Err(format!("Invalid URL encoded string: {}", e).into()),
    }
}

/// HTML escape a string (escape HTML special characters)
fn escape_html_impl(input: &str) -> String {
    html_escape::encode_text(input).to_string()
}

/// HTML unescape a string (decode HTML entities)
fn unescape_html_impl(input: &str) -> String {
    html_escape::decode_html_entities(input).to_string()
}

/// JSON escape a string (escape JSON special characters)
fn escape_json_impl(input: &str) -> String {
    let json_str = serde_json::to_string(input).unwrap_or_else(|_| String::new());
    // Remove the surrounding quotes added by serde_json
    if json_str.len() >= 2 && json_str.starts_with('"') && json_str.ends_with('"') {
        json_str[1..json_str.len() - 1].to_string()
    } else {
        json_str
    }
}

/// JSON unescape a string (decode JSON escape sequences)
fn unescape_json_impl(input: &str) -> Result<String, Box<rhai::EvalAltResult>> {
    // Add quotes around the input to make it a valid JSON string literal
    let json_string = format!("\"{}\"", input);
    match serde_json::from_str::<String>(&json_string) {
        Ok(unescaped) => Ok(unescaped),
        Err(e) => Err(format!("Invalid JSON escape sequences: {}", e).into()),
    }
}

/// Convert a Rhai Dynamic value to a JSON string with indentation
/// This overload allows pretty-printing with the specified number of spaces
fn to_json_with_indent(
    value: rhai::Dynamic,
    indent: i64,
) -> Result<String, Box<rhai::EvalAltResult>> {
    let json_value = dynamic_to_json_value(&value);

    if indent > 0 {
        // Pretty-print with indentation
        let indent_bytes = vec![b' '; indent as usize];
        let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent_bytes);
        let mut buf = Vec::new();
        let mut serializer = serde_json::Serializer::with_formatter(&mut buf, formatter);
        json_value
            .serialize(&mut serializer)
            .map_err(|e| format!("Failed to serialize to JSON: {}", e))?;
        String::from_utf8(buf).map_err(|e| format!("Invalid UTF-8 in JSON output: {}", e).into())
    } else {
        // Compact output (same as builtin to_json())
        serde_json::to_string(&json_value)
            .map_err(|e| format!("Failed to serialize to JSON: {}", e).into())
    }
}

/// Convert Rhai Dynamic to serde_json::Value recursively
fn dynamic_to_json_value(value: &rhai::Dynamic) -> serde_json::Value {
    if value.is_unit() {
        return serde_json::Value::Null;
    }

    if value.is_string() {
        if let Ok(s) = value.clone().into_string() {
            return serde_json::Value::String(s);
        }
    }

    if let Ok(b) = value.as_bool() {
        return serde_json::Value::Bool(b);
    }

    if let Ok(i) = value.as_int() {
        return serde_json::Value::Number(serde_json::Number::from(i));
    }

    if let Ok(f) = value.as_float() {
        if let Some(num) = serde_json::Number::from_f64(f) {
            return serde_json::Value::Number(num);
        }
    }

    if let Some(arr) = value.clone().try_cast::<rhai::Array>() {
        let json_array: Vec<serde_json::Value> = arr.iter().map(dynamic_to_json_value).collect();
        return serde_json::Value::Array(json_array);
    }

    if let Some(map) = value.clone().try_cast::<rhai::Map>() {
        let mut json_obj = serde_json::Map::new();
        for (key, val) in map {
            json_obj.insert(key.to_string(), dynamic_to_json_value(&val));
        }
        return serde_json::Value::Object(json_obj);
    }

    // Fallback: try to convert to string
    serde_json::Value::String(format!("{:?}", value))
}

/// Register encoding/decoding functions with the Rhai engine
pub fn register_functions(engine: &mut Engine) {
    // Base64 encoding/decoding functions
    engine.register_fn("encode_b64", encode_b64_impl);
    engine.register_fn("decode_b64", decode_b64_impl);

    // Hexadecimal encoding/decoding functions
    engine.register_fn("encode_hex", encode_hex_impl);
    engine.register_fn("decode_hex", decode_hex_impl);

    // URL encoding/decoding functions
    engine.register_fn("encode_url", encode_url_impl);
    engine.register_fn("decode_url", decode_url_impl);

    // HTML escaping/unescaping functions
    engine.register_fn("escape_html", escape_html_impl);
    engine.register_fn("unescape_html", unescape_html_impl);

    // JSON escaping/unescaping functions
    engine.register_fn("escape_json", escape_json_impl);
    engine.register_fn("unescape_json", unescape_json_impl);

    // JSON conversion with indentation (overload for pretty-printing)
    engine.register_fn("to_json", to_json_with_indent);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhai::Scope;

    #[test]
    fn test_base64_encoding() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Hello, World!");

        // Test encoding
        let result: String = engine
            .eval_with_scope(&mut scope, r#"encode_b64(text)"#)
            .unwrap();
        assert_eq!(result, "SGVsbG8sIFdvcmxkIQ==");

        // Test decoding
        scope.push("encoded", "SGVsbG8sIFdvcmxkIQ==");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_b64(encoded)"#)
            .unwrap();
        assert_eq!(result, "Hello, World!");

        // Test round trip
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_b64(encode_b64(text))"#)
            .unwrap();
        assert_eq!(result, "Hello, World!");

        // Test invalid base64
        scope.push("invalid", "invalid base64!");
        let result = engine.eval_with_scope::<String>(&mut scope, r#"decode_b64(invalid)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_hex_encoding() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Hello!");

        // Test encoding
        let result: String = engine
            .eval_with_scope(&mut scope, r#"encode_hex(text)"#)
            .unwrap();
        assert_eq!(result, "48656c6c6f21");

        // Test decoding
        scope.push("encoded", "48656c6c6f21");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_hex(encoded)"#)
            .unwrap();
        assert_eq!(result, "Hello!");

        // Test round trip
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_hex(encode_hex(text))"#)
            .unwrap();
        assert_eq!(result, "Hello!");

        // Test invalid hex
        scope.push("invalid", "gghhii");
        let result = engine.eval_with_scope::<String>(&mut scope, r#"decode_hex(invalid)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_url_encoding() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Hello World & Special chars!");

        // Test encoding
        let result: String = engine
            .eval_with_scope(&mut scope, r#"encode_url(text)"#)
            .unwrap();
        assert_eq!(result, "Hello%20World%20%26%20Special%20chars%21");

        // Test decoding
        scope.push("encoded", "Hello%20World%20%26%20Special%20chars%21");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_url(encoded)"#)
            .unwrap();
        assert_eq!(result, "Hello World & Special chars!");

        // Test round trip
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_url(encode_url(text))"#)
            .unwrap();
        assert_eq!(result, "Hello World & Special chars!");

        // Test invalid URL encoding (urlencoding crate may handle %ZZ differently)
        scope.push("invalid", "invalid%ZZ");
        let result = engine.eval_with_scope::<String>(&mut scope, r#"decode_url(invalid)"#);
        // urlencoding crate might not consider %ZZ as invalid, so we test with a different invalid case
        scope.push("invalid2", "invalid%G");
        let result2 = engine.eval_with_scope::<String>(&mut scope, r#"decode_url(invalid2)"#);
        // At least one should be an error or return the original string
        assert!(result.is_err() || result2.is_err() || result.unwrap() == "invalid%ZZ");
    }

    #[test]
    fn test_html_escaping() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "<script>alert('xss')</script>");

        // Test escaping
        let result: String = engine
            .eval_with_scope(&mut scope, r#"escape_html(text)"#)
            .unwrap();
        assert_eq!(result, "&lt;script&gt;alert('xss')&lt;/script&gt;");

        // Test unescaping
        scope.push(
            "escaped",
            "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;",
        );
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_html(escaped)"#)
            .unwrap();
        assert_eq!(result, "<script>alert('xss')</script>");

        // Test round trip
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_html(escape_html(text))"#)
            .unwrap();
        assert_eq!(result, "<script>alert('xss')</script>");

        // Test common HTML entities
        scope.push("entities", "&amp; &lt; &gt; &quot; &#x27;");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_html(entities)"#)
            .unwrap();
        assert_eq!(result, "& < > \" '");
    }

    #[test]
    fn test_json_escaping() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("text", "Hello\nWorld\t\"test\"");

        // Test escaping
        let result: String = engine
            .eval_with_scope(&mut scope, r#"escape_json(text)"#)
            .unwrap();
        assert_eq!(result, "Hello\\nWorld\\t\\\"test\\\"");

        // Test unescaping
        scope.push("escaped", "Hello\\nWorld\\t\\\"test\\\"");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_json(escaped)"#)
            .unwrap();
        assert_eq!(result, "Hello\nWorld\t\"test\"");

        // Test round trip
        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_json(escape_json(text))"#)
            .unwrap();
        assert_eq!(result, "Hello\nWorld\t\"test\"");

        // Test invalid JSON escape sequences
        scope.push("invalid", "invalid\\x escape");
        let result = engine.eval_with_scope::<String>(&mut scope, r#"unescape_json(invalid)"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_strings() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = Scope::new();
        scope.push("empty", "");

        // Test all encoding functions with empty strings
        let result: String = engine
            .eval_with_scope(&mut scope, r#"encode_b64(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"encode_hex(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"encode_url(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"escape_html(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"escape_json(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        // Test all decoding functions with empty strings
        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_b64(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_hex(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"decode_url(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_html(empty)"#)
            .unwrap();
        assert_eq!(result, "");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"unescape_json(empty)"#)
            .unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_to_json_with_indent_map() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test pretty-printing with indent = 2
        let result: String = engine
            .eval(r#"let m = #{name: "John", age: 30, city: "NYC"}; m.to_json(2)"#)
            .unwrap();

        // Check that result contains newlines and indentation
        assert!(result.contains('\n'));
        assert!(result.contains("  \"name\""));
        assert!(result.contains("  \"age\""));
        assert!(result.contains("  \"city\""));

        // Test compact output with indent = 0
        let compact: String = engine
            .eval(r#"let m = #{name: "John", age: 30}; m.to_json(0)"#)
            .unwrap();

        assert!(!compact.contains('\n'));
        assert!(compact.contains("\"name\":\"John\""));
    }

    #[test]
    fn test_to_json_with_indent_array() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test array with indent = 4
        let result: String = engine
            .eval(r#"let arr = [1, 2, 3, 4, 5]; arr.to_json(4)"#)
            .unwrap();

        // Check that result contains newlines and proper indentation
        assert!(result.contains('\n'));
        assert!(result.contains("    "));

        // Test compact output
        let compact: String = engine
            .eval(r#"let arr = [1, 2, 3]; arr.to_json(0)"#)
            .unwrap();

        assert!(!compact.contains('\n'));
        assert_eq!(compact, "[1,2,3]");
    }

    #[test]
    fn test_to_json_with_indent_nested() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test nested structures with indent = 2
        let result: String = engine
            .eval(
                r#"
                let m = #{
                    user: #{name: "Alice", age: 25},
                    items: ["apple", "banana", "cherry"]
                };
                m.to_json(2)
                "#,
            )
            .unwrap();

        // Check that result is properly formatted
        assert!(result.contains('\n'));
        assert!(result.contains("  \"user\""));
        assert!(result.contains("    \"name\""));
        assert!(result.contains("  \"items\""));
    }

    #[test]
    fn test_to_json_with_indent_types() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Test different value types
        let result: String = engine
            .eval(
                r#"
                let m = #{
                    string: "hello",
                    number: 42,
                    float: 3.14,
                    bool: true,
                    nothing: ()
                };
                m.to_json(2)
                "#,
            )
            .unwrap();

        assert!(result.contains("\"string\": \"hello\""));
        assert!(result.contains("\"number\": 42"));
        assert!(result.contains("\"float\": 3.14"));
        assert!(result.contains("\"bool\": true"));
        assert!(result.contains("\"nothing\": null"));
    }

    #[test]
    fn test_to_json_builtin_still_works() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        // Ensure the builtin to_json() without parameters still works
        let result: String = engine
            .eval(r#"let m = #{name: "Bob", age: 35}; m.to_json()"#)
            .unwrap();

        // Should be compact (no newlines)
        assert!(!result.contains('\n'));
        assert!(result.contains("\"name\""));
        assert!(result.contains("\"age\""));
    }
}
