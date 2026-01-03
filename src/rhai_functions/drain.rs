use crate::drain::{self, DrainConfig};
use crate::rhai_functions::strings::is_parallel_mode;
use rhai::{Dynamic, Engine, EvalAltResult, Map, Position};

fn ensure_sequential() -> Result<(), Box<EvalAltResult>> {
    if is_parallel_mode() {
        return Err(EvalAltResult::ErrorRuntime(
            "'drain' is not available in --parallel mode (requires sequential processing)".into(),
            Position::NONE,
        )
        .into());
    }
    Ok(())
}

fn parse_drain_config(options: Map) -> Result<DrainConfig, Box<EvalAltResult>> {
    let mut config = DrainConfig::default();
    for (key, value) in options {
        match key.as_ref() {
            "depth" => {
                let depth = value.as_int().map_err(|_| {
                    EvalAltResult::ErrorRuntime(
                        "drain_template depth must be an integer".into(),
                        Position::NONE,
                    )
                })?;
                if depth <= 0 {
                    return Err(EvalAltResult::ErrorRuntime(
                        "drain_template depth must be greater than 0".into(),
                        Position::NONE,
                    )
                    .into());
                }
                config.depth = depth as usize;
            }
            "max_children" => {
                let max_children = value.as_int().map_err(|_| {
                    EvalAltResult::ErrorRuntime(
                        "drain_template max_children must be an integer".into(),
                        Position::NONE,
                    )
                })?;
                if max_children <= 0 {
                    return Err(EvalAltResult::ErrorRuntime(
                        "drain_template max_children must be greater than 0".into(),
                        Position::NONE,
                    )
                    .into());
                }
                config.max_children = max_children as usize;
            }
            "similarity" => {
                let similarity = if value.is_float() {
                    value.as_float().unwrap_or(0.0)
                } else if value.is_int() {
                    value.as_int().unwrap_or(0) as f64
                } else {
                    return Err(EvalAltResult::ErrorRuntime(
                        "drain_template similarity must be numeric".into(),
                        Position::NONE,
                    )
                    .into());
                };
                config.similarity = similarity;
            }
            "filters" => {
                config.filters = parse_filters(value)?;
            }
            _ => {
                return Err(EvalAltResult::ErrorRuntime(
                    format!("drain_template unknown option '{}'", key).into(),
                    Position::NONE,
                )
                .into());
            }
        }
    }

    Ok(config)
}

fn parse_filters(value: Dynamic) -> Result<Vec<String>, Box<EvalAltResult>> {
    if value.is_string() {
        let s = value.into_string().map_err(|_| {
            EvalAltResult::ErrorRuntime(
                "drain_template filters must be a string or array".into(),
                Position::NONE,
            )
        })?;
        Ok(s.split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect())
    } else if value.is_array() {
        let arr = value.into_array().map_err(|_| {
            EvalAltResult::ErrorRuntime(
                "drain_template filters must be a string or array".into(),
                Position::NONE,
            )
        })?;
        arr.into_iter()
            .map(|item| {
                item.into_string().map_err(|_| {
                    EvalAltResult::ErrorRuntime(
                        "drain_template filters array must contain strings".into(),
                        Position::NONE,
                    )
                    .into()
                })
            })
            .collect()
    } else {
        Err(EvalAltResult::ErrorRuntime(
            "drain_template filters must be a string or array".into(),
            Position::NONE,
        )
        .into())
    }
}

fn drain_template_simple(text: &str) -> Result<Map, Box<EvalAltResult>> {
    ensure_sequential()?;
    let result = drain::drain_template(text, None)
        .map_err(|msg| EvalAltResult::ErrorRuntime(msg.into(), Position::NONE))?;
    Ok(drain_result_to_map(result))
}

fn drain_template_with_options(text: &str, options: Map) -> Result<Map, Box<EvalAltResult>> {
    ensure_sequential()?;
    let config = parse_drain_config(options)?;
    let result = drain::drain_template(text, Some(config))
        .map_err(|msg| EvalAltResult::ErrorRuntime(msg.into(), Position::NONE))?;
    Ok(drain_result_to_map(result))
}

fn drain_templates_list() -> Result<rhai::Array, Box<EvalAltResult>> {
    ensure_sequential()?;
    let templates = drain::drain_templates();
    let mut array = rhai::Array::with_capacity(templates.len());
    for template in templates {
        let mut map = Map::new();
        map.insert("template".into(), Dynamic::from(template.template));
        map.insert("template_id".into(), Dynamic::from(template.template_id));
        map.insert("count".into(), Dynamic::from(template.count as i64));
        array.push(Dynamic::from(map));
    }
    Ok(array)
}

fn drain_result_to_map(result: drain::DrainResult) -> Map {
    let mut map = Map::new();
    map.insert("template".into(), Dynamic::from(result.template));
    map.insert("template_id".into(), Dynamic::from(result.template_id));
    map.insert("count".into(), Dynamic::from(result.count as i64));
    map.insert("is_new".into(), Dynamic::from(result.is_new));
    map
}

fn drain_template_id(template: &str) -> String {
    drain::generate_template_id(template)
}

pub fn register_functions(engine: &mut Engine) {
    engine.register_fn("drain_template", drain_template_simple);
    engine.register_fn("drain_template", drain_template_with_options);
    engine.register_fn("drain_templates", drain_templates_list);
    engine.register_fn("drain_template_id", drain_template_id);
}
