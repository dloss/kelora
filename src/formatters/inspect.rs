use crate::event::Event;
use crate::formatters::utils::escape_for_display;
use crate::pipeline;

use rhai::Dynamic;

// Inspect formatter - detailed, type-aware introspection output
pub struct InspectFormatter {
    max_inline_chars: usize,
}

struct LineSpec<'a> {
    indent: usize,
    name: &'a str,
    name_width: usize,
    type_width: usize,
    type_label: &'a str,
    value_repr: &'a str,
}

impl InspectFormatter {
    const KEY_WIDTH_CAP: usize = 40;

    pub fn new(verbosity: u8) -> Self {
        // Gradually relax truncation with higher verbosity levels
        let max_inline_chars = match verbosity {
            0 => 80,
            1 => 160,
            _ => usize::MAX,
        };

        Self { max_inline_chars }
    }

    fn format_entries<'a, I>(&self, lines: &mut Vec<String>, entries: I, indent: usize)
    where
        I: IntoIterator<Item = (&'a str, &'a Dynamic)>,
    {
        // Collect to compute alignment without re-iterating source data
        let collected: Vec<(&str, &Dynamic)> = entries.into_iter().collect();
        if collected.is_empty() {
            return;
        }

        let name_width = self.compute_key_width(collected.iter().map(|(k, _)| *k));
        let type_width = self.compute_type_width(collected.iter().map(|(_, v)| *v));

        for (key, value) in collected {
            self.format_entry_with_width(lines, key, value, indent, name_width, type_width);
        }
    }

    fn format_entry_with_width(
        &self,
        lines: &mut Vec<String>,
        name: &str,
        value: &Dynamic,
        indent: usize,
        name_width: usize,
        type_width: usize,
    ) {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            let entries: Vec<(String, Dynamic)> =
                map.into_iter().map(|(k, v)| (k.into(), v)).collect();
            let type_label = format!("map({})", entries.len());
            self.push_line(
                lines,
                LineSpec {
                    indent,
                    name,
                    name_width,
                    type_width,
                    type_label: &type_label,
                    value_repr: "{",
                },
            );

            if !entries.is_empty() {
                let child_width = self.compute_key_width(entries.iter().map(|(k, _)| k.as_str()));
                let child_type_width = self.compute_type_width(entries.iter().map(|(_, v)| v));
                for (child_key, child_value) in &entries {
                    self.format_entry_with_width(
                        lines,
                        child_key,
                        child_value,
                        indent + 1,
                        child_width,
                        child_type_width,
                    );
                }
            }

            lines.push(format!("{}{}", "  ".repeat(indent), "}"));
        } else if let Some(array) = value.clone().try_cast::<rhai::Array>() {
            let elements: Vec<Dynamic> = array.into_iter().collect();
            let type_label = format!("array({})", elements.len());
            self.push_line(
                lines,
                LineSpec {
                    indent,
                    name,
                    name_width,
                    type_width,
                    type_label: &type_label,
                    value_repr: "[",
                },
            );

            if !elements.is_empty() {
                let index_labels: Vec<String> =
                    (0..elements.len()).map(|i| format!("[{}]", i)).collect();
                let child_width = self.compute_key_width(index_labels.iter().map(|s| s.as_str()));
                let child_type_width = self.compute_type_width(elements.iter());

                for (idx, element) in elements.iter().enumerate() {
                    let child_name = &index_labels[idx];
                    self.format_entry_with_width(
                        lines,
                        child_name,
                        element,
                        indent + 1,
                        child_width,
                        child_type_width,
                    );
                }
            }

            lines.push(format!("{}{}", "  ".repeat(indent), "]"));
        } else {
            let (type_label, value_repr) = self.describe_scalar(value);
            self.push_line(
                lines,
                LineSpec {
                    indent,
                    name,
                    name_width,
                    type_width,
                    type_label: &type_label,
                    value_repr: &value_repr,
                },
            );
        }
    }

    fn push_line(&self, lines: &mut Vec<String>, spec: LineSpec<'_>) {
        let indent_str = "  ".repeat(spec.indent);
        let name_cell = if spec.name_width > 0 {
            format!("{name:<width$}", name = spec.name, width = spec.name_width)
        } else {
            spec.name.to_string()
        };
        let effective_type_width = spec.type_width.max(spec.type_label.len());
        let type_cell = format!(
            "{type_label:<width$}",
            type_label = spec.type_label,
            width = effective_type_width
        );
        lines.push(format!(
            "{indent}{name_cell} | {type_cell} | {value}",
            indent = indent_str,
            name_cell = name_cell,
            type_cell = type_cell,
            value = spec.value_repr
        ));
    }

    fn compute_key_width<'a, I>(&self, keys: I) -> usize
    where
        I: Iterator<Item = &'a str>,
    {
        keys.map(|k| k.len())
            .max()
            .unwrap_or(0)
            .min(Self::KEY_WIDTH_CAP)
    }

    fn compute_type_width<'a, I>(&self, values: I) -> usize
    where
        I: Iterator<Item = &'a Dynamic>,
    {
        values
            .map(|value| self.type_label_for(value).len())
            .max()
            .unwrap_or(0)
    }

    fn type_label_for(&self, value: &Dynamic) -> String {
        if let Some(map) = value.clone().try_cast::<rhai::Map>() {
            return format!("map({})", map.len());
        }
        if let Some(array) = value.clone().try_cast::<rhai::Array>() {
            return format!("array({})", array.len());
        }
        if value.is_string() {
            return "string".to_string();
        }
        if value.is_bool() {
            return "bool".to_string();
        }
        if value.is_int() {
            return "int".to_string();
        }
        if value.is_float() {
            return "float".to_string();
        }
        if value.is_char() {
            return "char".to_string();
        }
        if value.is_unit() {
            return "null".to_string();
        }
        value.type_name().to_string()
    }

    fn describe_scalar(&self, value: &Dynamic) -> (String, String) {
        if value.is_string() {
            if let Ok(inner) = value.clone().into_string() {
                let escaped = escape_for_display(&inner);
                let (truncated, was_truncated) = self.truncate_value(&escaped);
                let mut rendered = format!("\"{}\"", truncated);
                if was_truncated {
                    rendered.push_str("...");
                }
                return ("string".to_string(), rendered);
            }
        }

        if value.is_bool() {
            if let Ok(b) = value.as_bool() {
                return ("bool".to_string(), b.to_string());
            }
        }

        if value.is_int() {
            if let Ok(i) = value.as_int() {
                return ("int".to_string(), i.to_string());
            }
        }

        if value.is_float() {
            if let Ok(f) = value.as_float() {
                return ("float".to_string(), format!("{f}"));
            }
        }

        if value.is_char() {
            if let Ok(c) = value.as_char() {
                return (
                    "char".to_string(),
                    format!("'{}'", escape_for_display(&c.to_string())),
                );
            }
        }

        if value.is_unit() {
            return ("null".to_string(), "null".to_string());
        }

        // Fallback for other scalar types
        let type_label = value.type_name().to_string();
        let rendered = escape_for_display(&value.to_string());
        let (truncated, was_truncated) = self.truncate_value(&rendered);
        let mut repr = truncated;
        if was_truncated {
            repr.push_str("...");
        }
        (type_label, repr)
    }

    fn truncate_value(&self, value: &str) -> (String, bool) {
        if self.max_inline_chars == usize::MAX || value.chars().count() <= self.max_inline_chars {
            return (value.to_string(), false);
        }

        let truncated: String = value.chars().take(self.max_inline_chars).collect();

        (truncated, true)
    }
}

impl pipeline::Formatter for InspectFormatter {
    fn format(&self, event: &Event) -> String {
        if event.fields.is_empty() {
            return "---".to_string();
        }

        let mut lines = Vec::new();
        self.format_entries(
            &mut lines,
            crate::event::ordered_fields(event)
                .into_iter()
                .map(|(k, v)| (k.as_str(), v)),
            0,
        );
        lines.insert(0, "---".to_string());
        lines.join("\n")
    }
}
