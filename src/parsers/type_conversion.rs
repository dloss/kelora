use rhai::Dynamic;
use std::collections::HashMap;

/// Supported type annotations for field conversion
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldType {
    Int,
    Float,
    Bool,
    String,
}

impl FieldType {
    /// Parse a type annotation string into a FieldType
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "int" | "i64" | "integer" => Some(FieldType::Int),
            "float" | "f64" | "double" => Some(FieldType::Float),
            "bool" | "boolean" => Some(FieldType::Bool),
            "string" | "str" => Some(FieldType::String),
            _ => None,
        }
    }
}

/// Type map for field name -> desired type
pub type TypeMap = HashMap<String, FieldType>;

/// Returns true if `s` is a syntactically valid JSON number (RFC 8259 §6):
///
/// ```text
/// number = [ "-" ] int [ frac ] [ exp ]
/// int    = "0" / ( digit1-9 *DIGIT )
/// frac   = "." 1*DIGIT
/// exp    = ("e" / "E") [ "+" / "-" ] 1*DIGIT
/// ```
///
/// This is the boundary Kelora uses to decide whether a bare value from a
/// type-inferring parser (logfmt, CEF) should be coerced to a number. It
/// deliberately rejects leading zeros (`007`), a leading `+`, and the
/// Rust-only spellings `inf`/`nan`/`infinity` that `f64::parse` would otherwise
/// accept. That keeps zero-padded IDs (zip codes, account numbers), signed
/// phone numbers, and version segments like `01` as strings instead of silently
/// rewriting them — and makes the same token resolve to the same type whether
/// it arrives on JSON input (where leading-zero numbers are illegal anyway) or
/// in a logfmt/CEF field, which matters for mixed-format cascades.
///
/// A value that passes this gate is not guaranteed to fit in `i64`; callers
/// still try `i64` then fall back to `f64` (e.g. for integers larger than
/// `i64::MAX`), exactly as before — this only decides *whether* to attempt a
/// numeric parse at all.
pub fn looks_like_json_number(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return false;
    }
    let mut i = 0;

    // optional minus (JSON allows only "-", never "+")
    if bytes[i] == b'-' {
        i += 1;
        if i == len {
            return false;
        }
    }

    // int part: "0" alone, or digit1-9 followed by digits (no leading zero)
    if bytes[i] == b'0' {
        i += 1;
    } else if bytes[i].is_ascii_digit() {
        i += 1;
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
    } else {
        return false;
    }

    // optional frac: "." then at least one digit
    if i < len && bytes[i] == b'.' {
        i += 1;
        if i == len || !bytes[i].is_ascii_digit() {
            return false;
        }
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }

    // optional exp: e/E, optional sign, then at least one digit
    if i < len && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < len && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        if i == len || !bytes[i].is_ascii_digit() {
            return false;
        }
        while i < len && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }

    i == len
}

/// Parse a field specification with optional type annotation
/// Returns (field_name, optional_type)
///
/// Examples:
/// - "status" -> ("status", None)
/// - "status:int" -> ("status", Some(FieldType::Int))
/// - "bytes:float" -> ("bytes", Some(FieldType::Float))
pub fn parse_field_with_type(spec: &str) -> Result<(String, Option<FieldType>), String> {
    let parts: Vec<&str> = spec.splitn(2, ':').collect();

    match parts.as_slice() {
        [field_name] => {
            // No type annotation
            Ok((field_name.trim().to_string(), None))
        }
        [field_name, type_str] => {
            // Type annotation present
            let field = field_name.trim().to_string();
            if field.is_empty() {
                return Err("Field name cannot be empty".to_string());
            }

            match FieldType::from_str(type_str.trim()) {
                Some(field_type) => Ok((field, Some(field_type))),
                None => Err(format!("Unknown type annotation: '{}'", type_str.trim())),
            }
        }
        _ => Err("Invalid field specification".to_string()),
    }
}

/// Convert a string value to a Dynamic with the specified type
///
/// # Arguments
/// * `value` - The string value to convert
/// * `field_type` - The target type
/// * `strict` - If true, return error on conversion failure; if false, the
///   field becomes `()` (unit / explicitly absent)
///
/// # Returns
/// * `Ok(Dynamic)` - Successfully converted value, or `()` on failure in
///   resilient mode
/// * `Err(String)` - Conversion error (only in strict mode)
///
/// Resilient mode deliberately yields `()` rather than the original string:
/// a `field:int` annotation declares the field's type, so a value that cannot
/// satisfy it is treated as absent (like a missing column) rather than silently
/// kept as a misleading number-shaped string. For tolerant coercion with a
/// chosen fallback, drop the annotation and use `to_int`/`to_int_or` in a
/// script stage instead.
pub fn convert_value_to_type(
    value: &str,
    field_type: &FieldType,
    strict: bool,
) -> Result<Dynamic, String> {
    match field_type {
        FieldType::Int => {
            match value.parse::<i64>() {
                Ok(i) => Ok(Dynamic::from(i)),
                Err(_) => {
                    if strict {
                        Err(format!("Cannot convert '{}' to int", value))
                    } else {
                        // Resilient mode: field becomes () (explicitly absent)
                        Ok(Dynamic::UNIT)
                    }
                }
            }
        }
        FieldType::Float => {
            match value.parse::<f64>() {
                Ok(f) => Ok(Dynamic::from(f)),
                Err(_) => {
                    if strict {
                        Err(format!("Cannot convert '{}' to float", value))
                    } else {
                        // Resilient mode: field becomes () (explicitly absent)
                        Ok(Dynamic::UNIT)
                    }
                }
            }
        }
        FieldType::Bool => {
            match value.to_lowercase().as_str() {
                "true" | "t" | "yes" | "y" | "1" => Ok(Dynamic::from(true)),
                "false" | "f" | "no" | "n" | "0" => Ok(Dynamic::from(false)),
                _ => {
                    if strict {
                        Err(format!("Cannot convert '{}' to bool", value))
                    } else {
                        // Resilient mode: field becomes () (explicitly absent)
                        Ok(Dynamic::UNIT)
                    }
                }
            }
        }
        FieldType::String => {
            // Always succeeds
            Ok(Dynamic::from(value.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_looks_like_json_number_accepts_valid_numbers() {
        for s in [
            "0",
            "-0",
            "5",
            "-5",
            "42",
            "123456789012345678",
            "1.5",
            "-1.5",
            "0.5",
            "-0.5",
            "1e3",
            "1E3",
            "1e+3",
            "1e-3",
            "1.5e10",
            "-2.5E-4",
            "100",
        ] {
            assert!(
                looks_like_json_number(s),
                "expected {s:?} to be a JSON number"
            );
        }
    }

    #[test]
    fn test_looks_like_json_number_rejects_non_json_numbers() {
        for s in [
            "",
            "007",       // leading zero
            "02134",     // zip code
            "01",        // version segment
            "00.5",      // leading-zero frac
            "+5",        // leading plus
            "+15551234", // signed phone number
            "inf",       // Rust-only float spelling
            "Infinity",  // ditto
            "-inf",      // ditto
            "nan",       // ditto
            "NaN",       // ditto
            "1_000",     // digit separators
            "0x1F",      // hex
            "1.",        // frac needs a digit
            ".5",        // int part required
            "1e",        // exp needs a digit
            "1e+",       // ditto
            "5abc",      // trailing garbage
            "- 5",       // space
            "--5",       // double sign
            "5.5.5",     // two dots
            "5-",        // trailing sign
        ] {
            assert!(
                !looks_like_json_number(s),
                "expected {s:?} NOT to be a JSON number"
            );
        }
    }

    #[test]
    fn test_field_type_from_str() {
        assert_eq!(FieldType::from_str("int"), Some(FieldType::Int));
        assert_eq!(FieldType::from_str("INT"), Some(FieldType::Int));
        assert_eq!(FieldType::from_str("i64"), Some(FieldType::Int));
        assert_eq!(FieldType::from_str("float"), Some(FieldType::Float));
        assert_eq!(FieldType::from_str("f64"), Some(FieldType::Float));
        assert_eq!(FieldType::from_str("bool"), Some(FieldType::Bool));
        assert_eq!(FieldType::from_str("string"), Some(FieldType::String));
        assert_eq!(FieldType::from_str("unknown"), None);
    }

    #[test]
    fn test_parse_field_with_type() {
        // Without type annotation
        let (name, type_opt) = parse_field_with_type("status").unwrap();
        assert_eq!(name, "status");
        assert_eq!(type_opt, None);

        // With type annotation
        let (name, type_opt) = parse_field_with_type("status:int").unwrap();
        assert_eq!(name, "status");
        assert_eq!(type_opt, Some(FieldType::Int));

        // With whitespace
        let (name, type_opt) = parse_field_with_type("  bytes : float  ").unwrap();
        assert_eq!(name, "bytes");
        assert_eq!(type_opt, Some(FieldType::Float));

        // Invalid type
        assert!(parse_field_with_type("field:unknown").is_err());

        // Empty field name
        assert!(parse_field_with_type(":int").is_err());
    }

    #[test]
    fn test_convert_value_to_type_int() {
        // Valid integer
        let result = convert_value_to_type("123", &FieldType::Int, true).unwrap();
        assert_eq!(result.as_int().unwrap(), 123);

        // Invalid integer - strict mode
        let result = convert_value_to_type("abc", &FieldType::Int, true);
        assert!(result.is_err());

        // Invalid integer - resilient mode: field becomes () (explicitly absent)
        let result = convert_value_to_type("abc", &FieldType::Int, false).unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn test_convert_value_to_type_float() {
        // Valid float
        let result = convert_value_to_type("123.45", &FieldType::Float, true).unwrap();
        assert!((result.as_float().unwrap() - 123.45).abs() < 0.001);

        // Invalid float - strict mode
        let result = convert_value_to_type("abc", &FieldType::Float, true);
        assert!(result.is_err());

        // Invalid float - resilient mode: field becomes () (explicitly absent)
        let result = convert_value_to_type("abc", &FieldType::Float, false).unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn test_convert_value_to_type_bool() {
        // Valid bool - true variants
        assert!(convert_value_to_type("true", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());
        assert!(convert_value_to_type("TRUE", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());
        assert!(convert_value_to_type("yes", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());
        assert!(convert_value_to_type("1", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());

        // Valid bool - false variants
        assert!(!convert_value_to_type("false", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());
        assert!(!convert_value_to_type("no", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());
        assert!(!convert_value_to_type("0", &FieldType::Bool, true)
            .unwrap()
            .as_bool()
            .unwrap());

        // Invalid bool - strict mode
        let result = convert_value_to_type("maybe", &FieldType::Bool, true);
        assert!(result.is_err());

        // Invalid bool - resilient mode: field becomes () (explicitly absent)
        let result = convert_value_to_type("maybe", &FieldType::Bool, false).unwrap();
        assert!(result.is_unit());
    }

    #[test]
    fn test_convert_value_to_type_string() {
        // String type always succeeds
        let result = convert_value_to_type("anything", &FieldType::String, true).unwrap();
        assert_eq!(result.clone().into_string().unwrap(), "anything");

        let result = convert_value_to_type("anything", &FieldType::String, false).unwrap();
        assert_eq!(result.clone().into_string().unwrap(), "anything");
    }

    proptest! {
        #[test]
        fn prop_convert_int_roundtrip(value in any::<i64>()) {
            let input = value.to_string();
            let converted = convert_value_to_type(&input, &FieldType::Int, true).unwrap();
            prop_assert_eq!(converted.as_int().unwrap(), value);
        }

        #[test]
        fn prop_convert_float_roundtrip(value in prop::num::f64::NORMAL) {
            let input = value.to_string();
            let converted = convert_value_to_type(&input, &FieldType::Float, true).unwrap();
            let parsed = converted.as_float().unwrap();
            prop_assert_eq!(parsed.to_bits(), value.to_bits());
        }

        #[test]
        fn prop_convert_int_non_strict_yields_unit_on_failure(input in "[A-Za-z!@#$%^&* ]+") {
            prop_assume!(input.parse::<i64>().is_err());
            // Resilient mode: an uncoercible value becomes () (explicitly absent),
            // not the original string.
            let converted = convert_value_to_type(&input, &FieldType::Int, false).unwrap();
            prop_assert!(converted.is_unit());
        }
    }
}
