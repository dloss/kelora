use rhai::{Array, Dynamic, Engine, Map};
use std::cell::Cell;

thread_local! {
    static PARSE_COLS_STRICT: Cell<bool> = const { Cell::new(false) };
}

/// Set strict parsing mode for parse_cols (controlled by pipeline config)
pub fn set_parse_cols_strict(strict: bool) {
    PARSE_COLS_STRICT.with(|flag| flag.set(strict));
}

fn is_parse_cols_strict() -> bool {
    PARSE_COLS_STRICT.with(|flag| flag.get())
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("parse_cols", parse_cols_whitespace);
    engine.register_fn("parse_cols", parse_cols_with_sep);
    engine.register_fn("parse_cols", parse_cols_array);
    engine.register_fn("parse_cols", parse_cols_array_with_sep);

    // Column extraction functions
    engine.register_fn("col", |text: &str, selector: &str| -> String {
        extract_columns(text, selector, " ", " ")
    });

    engine.register_fn("col", |text: &str, selector: &str, sep: &str| -> String {
        extract_columns(text, selector, sep, " ")
    });

    engine.register_fn(
        "col",
        |text: &str, selector: &str, sep: &str, outsep: &str| -> String {
            extract_columns(text, selector, sep, outsep)
        },
    );

    // Integer column functions (up to 6 columns)
    engine.register_fn("col", |text: &str, col: i64| -> String {
        let selector = col.to_string();
        extract_columns(text, &selector, " ", " ")
    });

    engine.register_fn("col", |text: &str, col: i64, sep: &str| -> String {
        let selector = col.to_string();
        extract_columns(text, &selector, sep, " ")
    });

    engine.register_fn("col", |text: &str, col1: i64, col2: i64| -> String {
        let selector = format!("{},{}", col1, col2);
        extract_columns(text, &selector, " ", " ")
    });

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, sep: &str| -> String {
            let selector = format!("{},{}", col1, col2);
            extract_columns(text, &selector, sep, " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64| -> String {
            let selector = format!("{},{},{}", col1, col2, col3);
            extract_columns(text, &selector, " ", " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64, sep: &str| -> String {
            let selector = format!("{},{},{}", col1, col2, col3);
            extract_columns(text, &selector, sep, " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64| -> String {
            let selector = format!("{},{},{},{}", col1, col2, col3, col4);
            extract_columns(text, &selector, " ", " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64, sep: &str| -> String {
            let selector = format!("{},{},{},{}", col1, col2, col3, col4);
            extract_columns(text, &selector, sep, " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64, col5: i64| -> String {
            let selector = format!("{},{},{},{},{}", col1, col2, col3, col4, col5);
            extract_columns(text, &selector, " ", " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64, col5: i64, sep: &str| -> String {
            let selector = format!("{},{},{},{},{}", col1, col2, col3, col4, col5);
            extract_columns(text, &selector, sep, " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64, col5: i64, col6: i64| -> String {
            let selector = format!("{},{},{},{},{},{}", col1, col2, col3, col4, col5, col6);
            extract_columns(text, &selector, " ", " ")
        },
    );

    engine.register_fn(
        "col",
        |text: &str,
         col1: i64,
         col2: i64,
         col3: i64,
         col4: i64,
         col5: i64,
         col6: i64,
         sep: &str|
         -> String {
            let selector = format!("{},{},{},{},{},{}", col1, col2, col3, col4, col5, col6);
            extract_columns(text, &selector, sep, " ")
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, selectors: rhai::Array| -> rhai::Array {
            selectors
                .into_iter()
                .filter_map(|s| s.into_string().ok())
                .map(|selector| Dynamic::from(extract_columns(text, &selector, " ", " ")))
                .collect()
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, selectors: rhai::Array, sep: &str| -> rhai::Array {
            selectors
                .into_iter()
                .filter_map(|s| s.into_string().ok())
                .map(|selector| Dynamic::from(extract_columns(text, &selector, sep, " ")))
                .collect()
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, selectors: rhai::Array, sep: &str, outsep: &str| -> rhai::Array {
            selectors
                .into_iter()
                .filter_map(|s| s.into_string().ok())
                .map(|selector| Dynamic::from(extract_columns(text, &selector, sep, outsep)))
                .collect()
        },
    );

    // Integer cols functions (up to 6 columns)
    engine.register_fn("cols", |text: &str, col: i64| -> rhai::Array {
        let selector = col.to_string();
        vec![Dynamic::from(extract_columns(text, &selector, " ", " "))]
    });

    engine.register_fn("cols", |text: &str, col: i64, sep: &str| -> rhai::Array {
        let selector = col.to_string();
        vec![Dynamic::from(extract_columns(text, &selector, sep, " "))]
    });

    engine.register_fn("cols", |text: &str, col1: i64, col2: i64| -> rhai::Array {
        vec![
            Dynamic::from(extract_columns(text, &col1.to_string(), " ", " ")),
            Dynamic::from(extract_columns(text, &col2.to_string(), " ", " ")),
        ]
    });

    engine.register_fn(
        "cols",
        |text: &str, col1: i64, col2: i64, sep: &str| -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), sep, " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, col1: i64, col2: i64, col3: i64| -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), " ", " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, col1: i64, col2: i64, col3: i64, sep: &str| -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), sep, " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64| -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col4.to_string(), " ", " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64, sep: &str| -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col4.to_string(), sep, " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str, col1: i64, col2: i64, col3: i64, col4: i64, col5: i64| -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col4.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col5.to_string(), " ", " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str,
         col1: i64,
         col2: i64,
         col3: i64,
         col4: i64,
         col5: i64,
         sep: &str|
         -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col4.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col5.to_string(), sep, " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str,
         col1: i64,
         col2: i64,
         col3: i64,
         col4: i64,
         col5: i64,
         col6: i64|
         -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col4.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col5.to_string(), " ", " ")),
                Dynamic::from(extract_columns(text, &col6.to_string(), " ", " ")),
            ]
        },
    );

    engine.register_fn(
        "cols",
        |text: &str,
         col1: i64,
         col2: i64,
         col3: i64,
         col4: i64,
         col5: i64,
         col6: i64,
         sep: &str|
         -> rhai::Array {
            vec![
                Dynamic::from(extract_columns(text, &col1.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col2.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col3.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col4.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col5.to_string(), sep, " ")),
                Dynamic::from(extract_columns(text, &col6.to_string(), sep, " ")),
            ]
        },
    );

    // Log analysis functions
    engine.register_fn("status_class", |status: i64| -> String {
        match status {
            100..=199 => "1xx".to_string(),
            200..=299 => "2xx".to_string(),
            300..=399 => "3xx".to_string(),
            400..=499 => "4xx".to_string(),
            500..=599 => "5xx".to_string(),
            _ => "unknown".to_string(),
        }
    });
}

pub fn parse_cols_whitespace(line: &str, spec: &str) -> Result<Map, Box<rhai::EvalAltResult>> {
    // Strip trailing newlines for consistency with other structured formats
    let line = line.trim_end_matches('\n').trim_end_matches('\r');
    let plan = parse_spec(spec).map_err(rhai_error)?;
    let (columns, byte_starts) = split_whitespace_columns(line);
    apply_spec(
        &plan,
        &columns,
        Some(&byte_starts),
        Some(line),
        JoinPolicy::Space,
        is_parse_cols_strict(),
    )
}

pub fn parse_cols_with_sep(
    line: &str,
    spec: &str,
    sep: &str,
) -> Result<Map, Box<rhai::EvalAltResult>> {
    if sep.is_empty() {
        return Err("parse_cols: separator must not be empty".into());
    }

    // Strip trailing newlines for consistency with other structured formats
    let line = line.trim_end_matches('\n').trim_end_matches('\r');
    let plan = parse_spec(spec).map_err(rhai_error)?;
    let (columns, byte_starts) = split_with_separator(line, sep);
    apply_spec(
        &plan,
        &columns,
        Some(&byte_starts),
        Some(line),
        JoinPolicy::Literal(sep),
        is_parse_cols_strict(),
    )
}

fn parse_cols_array(values: Array, spec: &str) -> Result<Map, Box<rhai::EvalAltResult>> {
    let plan = parse_spec(spec).map_err(rhai_error)?;

    let mut owned: Vec<String> = Vec::with_capacity(values.len());
    for value in values.into_iter() {
        let type_name = value.type_name().to_string();
        let string = value
            .into_string()
            .map_err(|_| -> Box<rhai::EvalAltResult> {
                format!(
                    "parse_cols: array elements must be strings (got {})",
                    type_name
                )
                .into()
            })?;
        owned.push(string);
    }

    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();

    apply_spec(
        &plan,
        &refs,
        None,
        None,
        JoinPolicy::ArraySpace,
        is_parse_cols_strict(),
    )
}

fn parse_cols_array_with_sep(
    values: Array,
    spec: &str,
    sep: &str,
) -> Result<Map, Box<rhai::EvalAltResult>> {
    let plan = parse_spec(spec).map_err(rhai_error)?;

    let mut owned: Vec<String> = Vec::with_capacity(values.len());
    for value in values.into_iter() {
        let type_name = value.type_name().to_string();
        let string = value
            .into_string()
            .map_err(|_| -> Box<rhai::EvalAltResult> {
                format!(
                    "parse_cols: array elements must be strings (got {})",
                    type_name
                )
                .into()
            })?;
        owned.push(string);
    }

    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();

    apply_spec(
        &plan,
        &refs,
        None,
        None,
        JoinPolicy::Literal(sep),
        is_parse_cols_strict(),
    )
}

fn apply_spec<'a>(
    plan: &SpecPlan,
    columns: &[&'a str],
    byte_starts: Option<&[usize]>,
    original_line: Option<&'a str>,
    join_policy: JoinPolicy<'a>,
    strict: bool,
) -> Result<Map, Box<rhai::EvalAltResult>> {
    let mut result = Map::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut shortage_detected = false;
    let mut index = 0usize;
    let total_columns = columns.len();

    for token in &plan.tokens {
        match token {
            SpecToken::Field { name, count } => {
                let available = total_columns.saturating_sub(index);
                let take = available.min(*count);

                if take > 0 {
                    let value = if *count == 1 && take == 1 {
                        Dynamic::from(columns[index].to_string())
                    } else {
                        Dynamic::from(join_values(&columns[index..index + take], &join_policy))
                    };
                    result.insert(name.clone().into(), value);
                } else {
                    result.insert(name.clone().into(), Dynamic::UNIT);
                }

                if take < *count {
                    shortage_detected = true;
                }

                index += take;
            }
            SpecToken::Skip { count } => {
                let available = total_columns.saturating_sub(index);
                if available < *count {
                    shortage_detected = true;
                    index = total_columns;
                } else {
                    index += *count;
                }
            }
            SpecToken::Rest { name } => {
                let value = if index < total_columns {
                    if let (Some(starts), Some(line)) = (byte_starts, original_line) {
                        if let Some(start_idx) = starts.get(index) {
                            if *start_idx < line.len() {
                                Dynamic::from(line[*start_idx..].to_string())
                            } else {
                                Dynamic::UNIT
                            }
                        } else {
                            Dynamic::UNIT
                        }
                    } else {
                        let remainder = &columns[index..];
                        if remainder.is_empty() {
                            Dynamic::UNIT
                        } else {
                            Dynamic::from(join_values(remainder, &join_policy))
                        }
                    }
                } else {
                    Dynamic::UNIT
                };

                result.insert(name.clone().into(), value);
                index = total_columns;
                break;
            }
        }
    }

    let need_min = plan.min_required;
    if total_columns < need_min {
        shortage_detected = true;
    }

    let consumed = index.min(total_columns);
    let extra = if plan.has_rest {
        0
    } else {
        total_columns.saturating_sub(consumed)
    };

    if shortage_detected {
        let message = format!(
            "parse_cols: expected >= {} columns (got {})",
            need_min, total_columns
        );
        if strict {
            return Err(message.into());
        }
        warnings.push(message);
    }

    if extra > 0 {
        let message = format!(
            "parse_cols: {} unconsumed columns; add *field or skip with -",
            extra
        );
        if strict {
            return Err(message.into());
        }
    }

    Ok(result)
}

fn split_whitespace_columns(line: &str) -> (Vec<&str>, Vec<usize>) {
    let bytes = line.as_bytes();
    let mut columns = Vec::new();
    let mut starts = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }

        if i >= bytes.len() {
            break;
        }

        let start = i;
        while i < bytes.len() && bytes[i] != b' ' && bytes[i] != b'\t' {
            i += 1;
        }
        let end = i;

        columns.push(&line[start..end]);
        starts.push(start);
    }

    (columns, starts)
}

fn split_with_separator<'a>(line: &'a str, sep: &str) -> (Vec<&'a str>, Vec<usize>) {
    let mut columns = Vec::new();
    let mut starts = Vec::new();
    let mut last = 0usize;

    for (idx, _) in line.match_indices(sep) {
        columns.push(&line[last..idx]);
        starts.push(last);
        last = idx + sep.len();
    }

    columns.push(&line[last..]);
    starts.push(last);

    (columns, starts)
}

fn rhai_error(message: String) -> Box<rhai::EvalAltResult> {
    message.into()
}

/// Validate a `cols:` spec without parsing a line.
///
/// Called from CLI/config validation so a malformed spec fails once at startup
/// (as a usage error) instead of once per input line at runtime. The returned
/// message is phrased for a CLI user, not for the Rhai runtime.
pub fn validate_cols_spec(spec: &str) -> Result<(), String> {
    // Commas are the cascade separator for `-f` itself (`-f json,line`), so a
    // comma-separated cols spec is the most likely first guess. Say so plainly
    // rather than reporting the whole list as one bad field name.
    if let Some(token) = spec.split_whitespace().find(|t| t.contains(',')) {
        return Err(format!(
            "'{}' contains a comma. cols fields are separated by spaces, not commas \
             — did you mean 'cols:{}'?",
            token,
            spec.replace(',', " ")
        ));
    }

    parse_spec(spec)
        .map(|_| ())
        .map_err(|e| e.strip_prefix("parse_cols: ").unwrap_or(&e).to_string())
}

fn parse_spec(spec: &str) -> Result<SpecPlan, String> {
    let mut tokens = Vec::new();
    let mut seen_rest = false;
    let mut min_required = 0usize;

    for raw_token in spec.split_whitespace() {
        if raw_token.is_empty() {
            continue;
        }

        if let Some(name) = raw_token.strip_prefix('*') {
            if seen_rest {
                return Err("parse_cols: *field may appear only once and must be last".to_string());
            }
            if name.is_empty() {
                return Err("parse_cols: *field requires a name".to_string());
            }
            if !is_valid_field_name(name) {
                return Err(invalid_field_name_message(name));
            }

            tokens.push(SpecToken::Rest {
                name: name.to_string(),
            });
            seen_rest = true;
            continue;
        }

        if seen_rest {
            return Err("parse_cols: *field must be the final token".to_string());
        }

        if raw_token == "-" {
            tokens.push(SpecToken::Skip { count: 1 });
            min_required += 1;
            continue;
        }

        if raw_token.starts_with("-(") && raw_token.ends_with(')') {
            let count = parse_count(&raw_token[2..raw_token.len() - 1], "skip")?;
            tokens.push(SpecToken::Skip { count });
            min_required += count;
            continue;
        }

        let (name, count) = parse_field_token(raw_token)?;
        min_required += count;
        tokens.push(SpecToken::Field { name, count });
    }

    if tokens.is_empty() {
        return Err("parse_cols: spec must contain at least one token".to_string());
    }

    let has_rest = matches!(tokens.last(), Some(SpecToken::Rest { .. }));
    if seen_rest && !has_rest {
        return Err("parse_cols: *field must be the final token".to_string());
    }

    Ok(SpecPlan {
        tokens,
        min_required,
        has_rest,
    })
}

fn parse_field_token(token: &str) -> Result<(String, usize), String> {
    if let Some(open) = token.find('(') {
        if !token.ends_with(')') {
            return Err(format!("parse_cols: invalid field token '{}'", token));
        }

        let name = &token[..open];
        let count_str = &token[open + 1..token.len() - 1];

        if name.is_empty() || !is_valid_field_name(name) {
            return Err(invalid_field_name_message(name));
        }

        let count = parse_count(count_str, "field")?;
        Ok((name.to_string(), count))
    } else {
        if !is_valid_field_name(token) {
            return Err(invalid_field_name_message(token));
        }
        Ok((token.to_string(), 1))
    }
}

fn parse_count(value: &str, kind: &str) -> Result<usize, String> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("parse_cols: invalid {} count '{}'", kind, value));
    }

    let count = value.parse::<usize>().unwrap_or(0);
    if count < 1 {
        return Err(format!(
            "parse_cols: {} count must be >= 1 (got {})",
            kind, count
        ));
    }
    Ok(count)
}

fn invalid_field_name_message(name: &str) -> String {
    format!(
        "parse_cols: invalid field name '{}' (field names may contain letters, digits and \
         underscores, and may not start with a digit)",
        name
    )
}

fn is_valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_alphabetic() => {}
        _ => return false,
    }

    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn join_values(slice: &[&str], policy: &JoinPolicy<'_>) -> String {
    match policy {
        JoinPolicy::Space | JoinPolicy::ArraySpace => slice.join(" "),
        JoinPolicy::Literal(sep) => slice.join(sep),
    }
}

#[derive(Debug)]
struct SpecPlan {
    tokens: Vec<SpecToken>,
    min_required: usize,
    has_rest: bool,
}

#[derive(Debug)]
enum SpecToken {
    Field { name: String, count: usize },
    Skip { count: usize },
    Rest { name: String },
}

enum JoinPolicy<'a> {
    Space,
    Literal(&'a str),
    ArraySpace,
}

/// Extract columns from text using selector syntax
fn extract_columns(text: &str, selector: &str, sep: &str, outsep: &str) -> String {
    let columns = split_text(text, sep);
    let indices = parse_selector(selector, columns.len());

    let selected: Vec<String> = indices
        .into_iter()
        .filter_map(|i| columns.get(i).map(|s| s.to_string()))
        .collect();

    selected.join(outsep)
}

/// Split text by separator, handling whitespace specially
fn split_text<'a>(text: &'a str, sep: &str) -> Vec<&'a str> {
    if sep == " " {
        // Whitespace splitting - split on any whitespace and filter empty
        text.split_whitespace().collect()
    } else {
        // Regular splitting
        text.split(sep).collect()
    }
}

/// Parse selector string into column indices
fn parse_selector(selector: &str, total_cols: usize) -> Vec<usize> {
    let mut indices = Vec::new();

    // Handle comma-separated selectors
    for part in selector.split(',') {
        let part = part.trim();

        if part.contains(':') {
            // Range selector like "1:3" or "2:" or ":4"
            let range_indices = parse_range_selector(part, total_cols);
            indices.extend(range_indices);
        } else {
            // Single index selector
            if let Ok(idx) = part.parse::<i32>() {
                if let Some(pos) = normalize_index(idx, total_cols) {
                    indices.push(pos);
                }
            }
        }
    }

    indices
}

/// Parse range selector like "1:3" into indices
fn parse_range_selector(range: &str, total_cols: usize) -> Vec<usize> {
    let parts: Vec<&str> = range.split(':').collect();
    if parts.len() != 2 {
        return Vec::new();
    }

    let start = if parts[0].trim().is_empty() {
        0
    } else {
        match parts[0].trim().parse::<i32>() {
            Ok(idx) => match normalize_index(idx, total_cols) {
                Some(normalized) => normalized,
                None => return Vec::new(), // Out of bounds start index
            },
            Err(_) => return Vec::new(),
        }
    };

    let end = if parts[1].trim().is_empty() {
        total_cols
    } else {
        match parts[1].trim().parse::<i32>() {
            Ok(idx) => {
                let len = total_cols as i32;
                let normalized = if idx < 0 { len + idx } else { idx };
                if normalized < 0 {
                    return Vec::new();
                }
                normalized.min(len) as usize
            }
            Err(_) => return Vec::new(),
        }
    };

    if start >= end {
        return Vec::new();
    }

    (start..end.min(total_cols)).collect()
}

#[cfg(test)]
mod parse_cols_tests {
    use super::*;
    use rhai::{Array, Dynamic};

    fn dynamic_to_string(value: &Dynamic) -> String {
        value.clone().into_string().unwrap()
    }

    #[test]
    fn parse_cols_whitespace_keeps_verbatim_tail() {
        set_parse_cols_strict(false);

        let map = parse_cols_whitespace(
            "2025-09-22 12:33:44 -- INFO hello   world",
            "ts(2) - level *msg",
        )
        .unwrap();

        assert_eq!(
            dynamic_to_string(map.get("ts").unwrap()),
            "2025-09-22 12:33:44"
        );
        assert_eq!(dynamic_to_string(map.get("level").unwrap()), "INFO");
        assert_eq!(dynamic_to_string(map.get("msg").unwrap()), "hello   world");
    }

    #[test]
    fn parse_cols_custom_separator_preserves_empty_columns() {
        set_parse_cols_strict(false);

        let map =
            parse_cols_with_sep("2025-09-22|12:34:56|INFO||done", "ts(2) level *msg", "|").unwrap();

        assert_eq!(
            dynamic_to_string(map.get("ts").unwrap()),
            "2025-09-22|12:34:56"
        );
        assert_eq!(dynamic_to_string(map.get("level").unwrap()), "INFO");
        assert_eq!(dynamic_to_string(map.get("msg").unwrap()), "|done");
    }

    #[test]
    fn parse_cols_array_resilient_shortage_sets_unit() {
        set_parse_cols_strict(false);

        let columns: Array = vec![
            Dynamic::from("2025-09-22"),
            Dynamic::from("INFO"),
            Dynamic::from("alice"),
        ];

        let map = parse_cols_array(columns, "ts level user action").unwrap();

        assert_eq!(dynamic_to_string(map.get("ts").unwrap()), "2025-09-22");
        assert!(map.get("action").unwrap().is_unit());
    }

    #[test]
    fn parse_cols_array_with_custom_separator() {
        set_parse_cols_strict(false);

        let columns: Array = vec![
            Dynamic::from("alpha"),
            Dynamic::from("beta"),
            Dynamic::from("gamma"),
            Dynamic::from("delta"),
            Dynamic::from("epsilon"),
        ];

        let map = parse_cols_array_with_sep(columns, "first second(2) *rest", "::").unwrap();

        assert_eq!(dynamic_to_string(map.get("first").unwrap()), "alpha");
        assert_eq!(dynamic_to_string(map.get("second").unwrap()), "beta::gamma");
        assert_eq!(
            dynamic_to_string(map.get("rest").unwrap()),
            "delta::epsilon"
        );
    }

    #[test]
    fn parse_cols_strict_shortage_errors() {
        set_parse_cols_strict(true);
        let result = parse_cols_whitespace("a b", "first second third");
        assert!(result.is_err());
        set_parse_cols_strict(false);
    }

    #[test]
    fn parse_cols_rejects_invalid_counts() {
        set_parse_cols_strict(false);
        let err = parse_cols_whitespace("hello", "field(0)").unwrap_err();
        assert!(err.to_string().contains("count"));
    }

    #[test]
    fn validate_cols_spec_accepts_valid_specs() {
        assert!(validate_cols_spec("ts level *msg").is_ok());
        assert!(validate_cols_spec("ts(2) level *msg").is_ok());
        assert!(validate_cols_spec("ts(2) - -(3) level *msg").is_ok());
        assert!(validate_cols_spec("single").is_ok());
    }

    #[test]
    fn validate_cols_spec_suggests_spaces_for_comma_lists() {
        let err = validate_cols_spec("ts,level,msg").unwrap_err();
        assert!(err.contains("separated by spaces, not commas"), "{}", err);
        assert!(err.contains("cols:ts level msg"), "{}", err);
    }

    #[test]
    fn validate_cols_spec_reports_bad_field_names_and_counts() {
        let err = validate_cols_spec("ts 9bad").unwrap_err();
        assert!(err.contains("9bad"), "{}", err);
        assert!(err.contains("may not start with a digit"), "{}", err);

        let err = validate_cols_spec("ts(0)").unwrap_err();
        assert!(err.contains("count"), "{}", err);
    }

    #[test]
    fn validate_cols_spec_rejects_misplaced_rest_token() {
        let err = validate_cols_spec("*msg level").unwrap_err();
        assert!(err.contains("final token"), "{}", err);
    }

    #[test]
    fn validate_cols_spec_messages_are_cli_phrased() {
        // The Rhai-facing prefix is stripped for CLI consumption.
        let err = validate_cols_spec("ts 9bad").unwrap_err();
        assert!(!err.starts_with("parse_cols:"), "{}", err);
    }
}

/// Convert potentially negative index to positive index
fn normalize_index(idx: i32, total_cols: usize) -> Option<usize> {
    let len = total_cols as i32;
    let normalized = if idx < 0 { len + idx } else { idx };

    if normalized >= 0 && normalized < len {
        Some(normalized as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_columns_basic() {
        assert_eq!(extract_columns("alpha beta gamma", "0", " ", " "), "alpha");
        assert_eq!(extract_columns("alpha beta gamma", "1", " ", " "), "beta");
        assert_eq!(extract_columns("alpha beta gamma", "2", " ", " "), "gamma");
        assert_eq!(extract_columns("alpha beta gamma", "-1", " ", " "), "gamma");
    }

    #[test]
    fn test_extract_columns_range() {
        assert_eq!(
            extract_columns("alpha beta gamma delta", "1:3", " ", " "),
            "beta gamma"
        );
        assert_eq!(
            extract_columns("alpha beta gamma delta", "2:", " ", " "),
            "gamma delta"
        );
        assert_eq!(
            extract_columns("alpha beta gamma delta", ":2", " ", " "),
            "alpha beta"
        );
    }

    #[test]
    fn test_extract_columns_multiple() {
        assert_eq!(
            extract_columns("alpha beta gamma delta", "0,2", " ", " "),
            "alpha gamma"
        );
        assert_eq!(
            extract_columns("alpha beta gamma delta", "1,3", " ", " "),
            "beta delta"
        );
        assert_eq!(
            extract_columns("alpha beta gamma delta", "0,2,3", " ", " "),
            "alpha gamma delta"
        );
    }

    #[test]
    fn test_extract_columns_custom_separator() {
        assert_eq!(extract_columns("alpha|beta|gamma", "0", "|", " "), "alpha");
        assert_eq!(
            extract_columns("alpha|beta|gamma", "1:3", "|", "|"),
            "beta|gamma"
        );
        assert_eq!(
            extract_columns("alpha,beta,gamma", "0,2", ",", "-"),
            "alpha-gamma"
        );
    }

    #[test]
    fn test_extract_columns_out_of_bounds() {
        assert_eq!(extract_columns("alpha beta", "5", " ", " "), "");

        // This should only extract column 0 since column 5 doesn't exist
        let result = extract_columns("alpha beta", "0,5", " ", " ");
        assert_eq!(result, "alpha");

        let range_result = extract_columns("alpha beta", "5:7", " ", " ");
        assert_eq!(range_result, "");
    }

    #[test]
    fn test_split_text_whitespace() {
        let result = split_text("  alpha   beta   gamma  ", " ");
        assert_eq!(result, vec!["alpha", "beta", "gamma"]);
    }

    #[test]
    fn test_split_text_custom() {
        let result = split_text("alpha|beta||gamma", "|");
        assert_eq!(result, vec!["alpha", "beta", "", "gamma"]);
    }

    #[test]
    fn test_parse_selector_single() {
        assert_eq!(parse_selector("0", 3), vec![0]);
        assert_eq!(parse_selector("2", 3), vec![2]);
        assert_eq!(parse_selector("-1", 3), vec![2]);
    }

    #[test]
    fn test_parse_selector_range() {
        assert_eq!(parse_selector("1:3", 5), vec![1, 2]);
        assert_eq!(parse_selector("2:", 5), vec![2, 3, 4]);
        assert_eq!(parse_selector(":2", 5), vec![0, 1]);
    }

    #[test]
    fn test_parse_selector_multiple() {
        assert_eq!(parse_selector("0,2", 5), vec![0, 2]);
        assert_eq!(parse_selector("1,3,4", 5), vec![1, 3, 4]);
    }

    #[test]
    fn test_normalize_index() {
        assert_eq!(normalize_index(0, 3), Some(0));
        assert_eq!(normalize_index(2, 3), Some(2));
        assert_eq!(normalize_index(-1, 3), Some(2));
        assert_eq!(normalize_index(-3, 3), Some(0));
        assert_eq!(normalize_index(3, 3), None);
        assert_eq!(normalize_index(-4, 3), None);
    }

    #[test]
    fn test_col_function_integration() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = rhai::Scope::new();
        scope.push("line", "alpha beta gamma delta");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col("0")"#)
            .unwrap();
        assert_eq!(result, "alpha");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col("1:3")"#)
            .unwrap();
        assert_eq!(result, "beta gamma");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col("0,2")"#)
            .unwrap();
        assert_eq!(result, "alpha gamma");
    }

    #[test]
    fn test_cols_function_integration() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = rhai::Scope::new();
        scope.push("line", "alpha beta gamma delta");

        // Test single selector
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"line.cols(["0"])"#)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].clone().into_string().unwrap(), "alpha");

        // Test multiple selectors
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"line.cols(["0", "2"])"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].clone().into_string().unwrap(), "alpha");
        assert_eq!(result[1].clone().into_string().unwrap(), "gamma");

        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"line.cols(["1:3", "-1"])"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].clone().into_string().unwrap(), "beta gamma");
        assert_eq!(result[1].clone().into_string().unwrap(), "delta");
    }

    #[test]
    fn test_col_integer_functions() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = rhai::Scope::new();
        scope.push("line", "alpha beta gamma delta");

        // Single integer
        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col(0)"#)
            .unwrap();
        assert_eq!(result, "alpha");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col(2)"#)
            .unwrap();
        assert_eq!(result, "gamma");

        // Multiple integers
        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col(0, 2)"#)
            .unwrap();
        assert_eq!(result, "alpha gamma");

        let result: String = engine
            .eval_with_scope(&mut scope, r#"line.col(1, 2, 3)"#)
            .unwrap();
        assert_eq!(result, "beta gamma delta");

        // With custom separator
        scope.push("csv_line", "alpha,beta,gamma,delta");
        let result: String = engine
            .eval_with_scope(&mut scope, r#"csv_line.col(0, 2, ",")"#)
            .unwrap();
        assert_eq!(result, "alpha gamma");
    }

    #[test]
    fn test_cols_integer_functions() {
        let mut engine = rhai::Engine::new();
        register_functions(&mut engine);

        let mut scope = rhai::Scope::new();
        scope.push("line", "alpha beta gamma delta");

        // Single integer
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"line.cols(0)"#)
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].clone().into_string().unwrap(), "alpha");

        // Multiple integers
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"line.cols(0, 2)"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].clone().into_string().unwrap(), "alpha");
        assert_eq!(result[1].clone().into_string().unwrap(), "gamma");

        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"line.cols(1, 2, 3)"#)
            .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].clone().into_string().unwrap(), "beta");
        assert_eq!(result[1].clone().into_string().unwrap(), "gamma");
        assert_eq!(result[2].clone().into_string().unwrap(), "delta");

        // With custom separator
        scope.push("csv_line", "alpha,beta,gamma,delta");
        let result: rhai::Array = engine
            .eval_with_scope(&mut scope, r#"csv_line.cols(0, 2, ",")"#)
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].clone().into_string().unwrap(), "alpha");
        assert_eq!(result[1].clone().into_string().unwrap(), "gamma");
    }
}
