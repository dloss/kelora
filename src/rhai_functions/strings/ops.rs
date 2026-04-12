use rhai::Engine;

mod output {
    use rhai::{Dynamic, Engine};

    use crate::rhai_functions::capture::{
        capture_eprint, capture_stderr, is_parallel_mode, is_suppress_side_effects,
    };

    pub fn register_functions(engine: &mut Engine) {
        engine.register_fn("eprint", |message: Dynamic| {
            if is_suppress_side_effects() {
                return;
            }

            let msg = message.to_string();
            if is_parallel_mode() {
                capture_eprint(msg.clone());
                capture_stderr(msg);
            } else {
                eprintln!("{}", msg);
            }
        });
    }
}

mod core {
    use rhai::{Array, Dynamic, Engine};
    use std::convert::TryFrom;

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

    pub fn register_functions(engine: &mut Engine) {
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

        engine.register_fn("or_empty", |text: &str| -> rhai::Dynamic {
            if text.is_empty() {
                Dynamic::UNIT
            } else {
                Dynamic::from(text.to_string())
            }
        });

        engine.register_fn("or_empty", |_unit: ()| -> rhai::Dynamic { Dynamic::UNIT });

        engine.register_fn("or_empty", |arr: rhai::Array| -> rhai::Dynamic {
            if arr.is_empty() {
                Dynamic::UNIT
            } else {
                Dynamic::from(arr)
            }
        });

        engine.register_fn("or_empty", |map: rhai::Map| -> rhai::Dynamic {
            if map.is_empty() {
                Dynamic::UNIT
            } else {
                Dynamic::from(map)
            }
        });

        engine.register_fn("lower", |text: &str| -> String { text.to_lowercase() });

        engine.register_fn("upper", |text: &str| -> String { text.to_uppercase() });

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

        engine.register_fn("join", |separator: &str, items: Array| -> String {
            items
                .into_iter()
                .filter_map(|item| item.into_string().ok())
                .collect::<Vec<String>>()
                .join(separator)
        });

        engine.register_fn("join", |items: Array, separator: &str| -> String {
            items
                .into_iter()
                .filter_map(|item| item.into_string().ok())
                .collect::<Vec<String>>()
                .join(separator)
        });
    }
}

mod slice {
    use rhai::Engine;

    pub fn register_functions(engine: &mut Engine) {
        engine.register_fn("slice", |s: &str, spec: &str| -> String {
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len() as i32;

            if len == 0 {
                return String::new();
            }

            let parts: Vec<&str> = spec.split(':').collect();

            let step = if parts.len() > 2 && !parts[2].trim().is_empty() {
                parts[2].trim().parse::<i32>().unwrap_or(1)
            } else {
                1
            };

            if step == 0 {
                return String::new();
            }

            let (default_start, default_end) = if step > 0 { (0, len) } else { (len - 1, -1) };

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
    }
}

mod substring {
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
}

mod trim {
    use rhai::Engine;
    use std::collections::HashSet;

    pub fn register_functions(engine: &mut Engine) {
        engine.register_fn("strip", |text: &str| -> String { text.trim().to_string() });

        engine.register_fn("strip", |text: &str, chars: &str| -> String {
            let chars_to_remove: HashSet<char> = chars.chars().collect();
            text.trim_matches(|c: char| chars_to_remove.contains(&c))
                .to_string()
        });

        engine.register_fn("lstrip", |text: &str| -> String {
            text.trim_start().to_string()
        });

        engine.register_fn("lstrip", |text: &str, chars: &str| -> String {
            let chars_to_remove: HashSet<char> = chars.chars().collect();
            text.trim_start_matches(|c: char| chars_to_remove.contains(&c))
                .to_string()
        });

        engine.register_fn("rstrip", |text: &str| -> String {
            text.trim_end().to_string()
        });

        engine.register_fn("rstrip", |text: &str, chars: &str| -> String {
            let chars_to_remove: HashSet<char> = chars.chars().collect();
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
    }
}

pub fn register_functions(engine: &mut Engine) {
    output::register_functions(engine);

    core::register_functions(engine);

    slice::register_functions(engine);

    substring::register_functions(engine);

    trim::register_functions(engine);
}
