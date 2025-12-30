use rhai::Engine;

pub fn register_functions(engine: &mut Engine) {
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

        let mut positions = Vec::new();
        let mut start = 0;
        while let Some(pos) = text[start..].find(substring) {
            positions.push(start + pos);
            start += pos + substring.len();
        }

        if positions.is_empty() {
            return String::new();
        }

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
            nth_usize - 1
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

            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(substring) {
                positions.push(start + pos);
                start += pos + substring.len();
            }

            if positions.is_empty() {
                return String::new();
            }

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
                nth_usize - 1
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

            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(start_substring) {
                positions.push(start + pos);
                start += pos + start_substring.len();
            }

            if positions.is_empty() {
                return String::new();
            }

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
                nth_usize - 1
            };

            let start_pos = positions[idx];
            let start_idx = start_pos + start_substring.len();
            let remainder = &text[start_idx..];

            if end_substring.is_empty() {
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

            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(prefix) {
                positions.push(start + pos);
                start += pos + prefix.len();
            }

            if positions.is_empty() {
                return String::new();
            }

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
                nth_usize - 1
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

            let mut positions = Vec::new();
            let mut start = 0;
            while let Some(pos) = text[start..].find(suffix) {
                positions.push(start + pos);
                start += pos + suffix.len();
            }

            if positions.is_empty() {
                return String::new();
            }

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
                nth_usize - 1
            };

            let pos = positions[idx];
            text[..pos + suffix.len()].to_string()
        },
    );
}
