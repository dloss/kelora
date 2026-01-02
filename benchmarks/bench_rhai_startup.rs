use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use rhai::{Dynamic, Engine, Scope};

use kelora::engine::RhaiEngine;
use kelora::event::Event;

/// Create a minimal test event with typical fields
fn make_test_event() -> Event {
    let mut event = Event::default_with_line("2024-01-15T10:30:45.123Z INFO [main] User login successful user_id=12345 session_id=abc-def-123".to_string());
    event.line_num = Some(1);
    event.filename = Some("/var/log/app.log".to_string());
    event.fields.insert("level".into(), Dynamic::from("INFO"));
    event.fields.insert("message".into(), Dynamic::from("User login successful"));
    event.fields.insert("user_id".into(), Dynamic::from(12345_i64));
    event.fields.insert("session_id".into(), Dynamic::from("abc-def-123"));
    event.fields.insert("timestamp".into(), Dynamic::from("2024-01-15T10:30:45.123Z"));
    event.fields.insert("logger".into(), Dynamic::from("main"));
    event
}

/// Create an event with many fields (stress test)
fn make_large_event() -> Event {
    let mut event = make_test_event();
    for i in 0..50 {
        event.fields.insert(format!("field_{}", i).into(), Dynamic::from(i as i64));
    }
    event
}

// =============================================================================
// One-time startup costs (useful context, though not per-event)
// =============================================================================

fn bench_engine_new_raw(c: &mut Criterion) {
    c.bench_function("engine_new_raw", |b| {
        b.iter(|| {
            black_box(Engine::new());
        });
    });
}

fn bench_rhai_engine_new(c: &mut Criterion) {
    c.bench_function("rhai_engine_new", |b| {
        b.iter(|| {
            black_box(RhaiEngine::new());
        });
    });
}

fn bench_function_registration(c: &mut Criterion) {
    c.bench_function("function_registration", |b| {
        b.iter(|| {
            let mut engine = Engine::new();
            kelora::rhai_functions::register_all_functions(&mut engine);
            black_box(engine);
        });
    });
}

// =============================================================================
// AST compilation (one-time per script, but important for startup)
// =============================================================================

fn bench_compile_simple_filter(c: &mut Criterion) {
    let engine = Engine::new();
    let filter = r#"e.level == "ERROR""#;
    c.bench_function("compile_simple_filter", |b| {
        b.iter(|| {
            black_box(engine.compile_expression(black_box(filter)).unwrap());
        });
    });
}

fn bench_compile_complex_filter(c: &mut Criterion) {
    let engine = Engine::new();
    let filter = r#"e.level == "ERROR" || (e.level == "WARN" && e.user_id > 1000 && e.message.contains("timeout"))"#;
    c.bench_function("compile_complex_filter", |b| {
        b.iter(|| {
            black_box(engine.compile_expression(black_box(filter)).unwrap());
        });
    });
}

fn bench_compile_exec_script(c: &mut Criterion) {
    let engine = Engine::new();
    let script = r#"
        e.processed = true;
        e.level_num = if e.level == "ERROR" { 3 } else if e.level == "WARN" { 2 } else { 1 };
        e.short_msg = e.message.sub_string(0, 20);
    "#;
    c.bench_function("compile_exec_script", |b| {
        b.iter(|| {
            black_box(engine.compile(black_box(script)).unwrap());
        });
    });
}

// =============================================================================
// Per-event overhead: Scope operations
// =============================================================================

fn bench_scope_new(c: &mut Criterion) {
    c.bench_function("scope_new", |b| {
        b.iter(|| {
            black_box(Scope::new());
        });
    });
}

fn bench_scope_clone_empty(c: &mut Criterion) {
    let scope = Scope::new();
    c.bench_function("scope_clone_empty", |b| {
        b.iter(|| {
            black_box(scope.clone());
        });
    });
}

fn bench_scope_clone_template(c: &mut Criterion) {
    let mut scope = Scope::new();
    scope.push("line", "");
    scope.push("e", rhai::Map::new());
    scope.push("meta", rhai::Map::new());
    scope.push("conf", rhai::Map::new());
    c.bench_function("scope_clone_template", |b| {
        b.iter(|| {
            black_box(scope.clone());
        });
    });
}

fn bench_scope_set_value_string(c: &mut Criterion) {
    let mut scope = Scope::new();
    scope.push("line", "");
    let line = "2024-01-15T10:30:45.123Z INFO test message";
    c.bench_function("scope_set_value_string", |b| {
        b.iter(|| {
            scope.set_value("line", black_box(line.to_string()));
        });
    });
}

fn bench_scope_set_value_map(c: &mut Criterion) {
    let mut scope = Scope::new();
    scope.push("e", rhai::Map::new());
    let mut map = rhai::Map::new();
    map.insert("level".into(), Dynamic::from("INFO"));
    map.insert("message".into(), Dynamic::from("test"));
    c.bench_function("scope_set_value_map", |b| {
        b.iter(|| {
            scope.set_value("e", black_box(map.clone()));
        });
    });
}

// =============================================================================
// Per-event overhead: Map building
// =============================================================================

fn bench_build_event_map_small(c: &mut Criterion) {
    let event = make_test_event();
    c.bench_function("build_event_map_small", |b| {
        b.iter(|| {
            let mut event_map = rhai::Map::new();
            for (k, v) in &event.fields {
                event_map.insert(k.clone().into(), v.clone());
            }
            black_box(event_map);
        });
    });
}

fn bench_build_event_map_large(c: &mut Criterion) {
    let event = make_large_event();
    c.bench_function("build_event_map_large", |b| {
        b.iter(|| {
            let mut event_map = rhai::Map::new();
            for (k, v) in &event.fields {
                event_map.insert(k.clone().into(), v.clone());
            }
            black_box(event_map);
        });
    });
}

fn bench_build_meta_map(c: &mut Criterion) {
    let event = make_test_event();
    c.bench_function("build_meta_map", |b| {
        b.iter(|| {
            let mut meta_map = rhai::Map::new();
            if let Some(line_num) = event.line_num {
                meta_map.insert("line_num".into(), Dynamic::from(line_num as i64));
            }
            if let Some(ref filename) = event.filename {
                meta_map.insert("filename".into(), Dynamic::from(filename.clone()));
            }
            meta_map.insert("line".into(), Dynamic::from(event.original_line.clone()));
            black_box(meta_map);
        });
    });
}

// =============================================================================
// Per-event overhead: Full scope creation (simulated create_scope_for_event)
// =============================================================================

fn bench_full_scope_creation_small(c: &mut Criterion) {
    let event = make_test_event();
    let mut scope_template = Scope::new();
    scope_template.push("line", "");
    scope_template.push("e", rhai::Map::new());
    scope_template.push("meta", rhai::Map::new());
    scope_template.push("conf", rhai::Map::new());

    c.bench_function("full_scope_creation_small", |b| {
        b.iter(|| {
            let mut scope = scope_template.clone();

            // Set line
            scope.set_value("line", event.original_line.clone());

            // Build and set event map
            let mut event_map = rhai::Map::new();
            for (k, v) in &event.fields {
                event_map.insert(k.clone().into(), v.clone());
            }
            scope.set_value("e", event_map);

            // Build and set meta map
            let mut meta_map = rhai::Map::new();
            if let Some(line_num) = event.line_num {
                meta_map.insert("line_num".into(), Dynamic::from(line_num as i64));
            }
            if let Some(ref filename) = event.filename {
                meta_map.insert("filename".into(), Dynamic::from(filename.clone()));
            }
            meta_map.insert("line".into(), Dynamic::from(event.original_line.clone()));
            scope.set_value("meta", meta_map);

            black_box(scope);
        });
    });
}

fn bench_full_scope_creation_large(c: &mut Criterion) {
    let event = make_large_event();
    let mut scope_template = Scope::new();
    scope_template.push("line", "");
    scope_template.push("e", rhai::Map::new());
    scope_template.push("meta", rhai::Map::new());
    scope_template.push("conf", rhai::Map::new());

    c.bench_function("full_scope_creation_large", |b| {
        b.iter(|| {
            let mut scope = scope_template.clone();

            scope.set_value("line", event.original_line.clone());

            let mut event_map = rhai::Map::new();
            for (k, v) in &event.fields {
                event_map.insert(k.clone().into(), v.clone());
            }
            scope.set_value("e", event_map);

            let mut meta_map = rhai::Map::new();
            if let Some(line_num) = event.line_num {
                meta_map.insert("line_num".into(), Dynamic::from(line_num as i64));
            }
            if let Some(ref filename) = event.filename {
                meta_map.insert("filename".into(), Dynamic::from(filename.clone()));
            }
            meta_map.insert("line".into(), Dynamic::from(event.original_line.clone()));
            scope.set_value("meta", meta_map);

            black_box(scope);
        });
    });
}

// =============================================================================
// Per-event overhead: Script evaluation
// =============================================================================

fn bench_eval_simple_filter(c: &mut Criterion) {
    let engine = Engine::new();
    let ast = engine.compile_expression(r#"e.level == "ERROR""#).unwrap();

    let mut scope_template = Scope::new();
    scope_template.push("line", "test line");
    let mut e_map = rhai::Map::new();
    e_map.insert("level".into(), Dynamic::from("ERROR"));
    scope_template.push("e", e_map);

    c.bench_function("eval_simple_filter", |b| {
        b.iter(|| {
            let mut scope = scope_template.clone();
            black_box(engine.eval_ast_with_scope::<bool>(&mut scope, &ast).unwrap());
        });
    });
}

fn bench_eval_complex_filter(c: &mut Criterion) {
    let engine = Engine::new();
    let ast = engine.compile_expression(
        r#"e.level == "ERROR" || (e.level == "WARN" && e.user_id > 1000)"#
    ).unwrap();

    let mut scope_template = Scope::new();
    scope_template.push("line", "test line");
    let mut e_map = rhai::Map::new();
    e_map.insert("level".into(), Dynamic::from("WARN"));
    e_map.insert("user_id".into(), Dynamic::from(1500_i64));
    scope_template.push("e", e_map);

    c.bench_function("eval_complex_filter", |b| {
        b.iter(|| {
            let mut scope = scope_template.clone();
            black_box(engine.eval_ast_with_scope::<bool>(&mut scope, &ast).unwrap());
        });
    });
}

fn bench_eval_with_string_method(c: &mut Criterion) {
    let engine = Engine::new();
    let ast = engine.compile_expression(
        r#"e.message.contains("error") || e.message.starts_with("FATAL")"#
    ).unwrap();

    let mut scope_template = Scope::new();
    scope_template.push("line", "test line");
    let mut e_map = rhai::Map::new();
    e_map.insert("message".into(), Dynamic::from("This is an error message"));
    scope_template.push("e", e_map);

    c.bench_function("eval_with_string_method", |b| {
        b.iter(|| {
            let mut scope = scope_template.clone();
            black_box(engine.eval_ast_with_scope::<bool>(&mut scope, &ast).unwrap());
        });
    });
}

// =============================================================================
// End-to-end: Full per-event processing simulation
// =============================================================================

fn bench_e2e_filter_simple(c: &mut Criterion) {
    let engine = Engine::new();
    let ast = engine.compile_expression(r#"e.level == "ERROR""#).unwrap();
    let event = make_test_event();

    let mut scope_template = Scope::new();
    scope_template.push("line", "");
    scope_template.push("e", rhai::Map::new());
    scope_template.push("meta", rhai::Map::new());
    scope_template.push("conf", rhai::Map::new());

    c.bench_function("e2e_filter_simple", |b| {
        b.iter(|| {
            // Full per-event cycle: scope creation + evaluation
            let mut scope = scope_template.clone();
            scope.set_value("line", event.original_line.clone());

            let mut event_map = rhai::Map::new();
            for (k, v) in &event.fields {
                event_map.insert(k.clone().into(), v.clone());
            }
            scope.set_value("e", event_map);

            black_box(engine.eval_ast_with_scope::<bool>(&mut scope, &ast).unwrap());
        });
    });
}

fn bench_e2e_filter_with_kelora_engine(c: &mut Criterion) {
    // This benchmark tests the overhead of the full Kelora RhaiEngine creation
    // which includes all custom function registrations
    c.bench_function("e2e_filter_with_kelora_engine_setup", |b| {
        b.iter(|| {
            let mut rhai_engine = RhaiEngine::new();
            rhai_engine.compile_filter(r#"e.level == "ERROR""#).unwrap();
            black_box(rhai_engine);
        });
    });
}

// =============================================================================
// Breakdown comparison: scope_creation vs eval
// =============================================================================

fn bench_breakdown_scope_only(c: &mut Criterion) {
    let event = make_test_event();
    let mut scope_template = Scope::new();
    scope_template.push("line", "");
    scope_template.push("e", rhai::Map::new());
    scope_template.push("meta", rhai::Map::new());
    scope_template.push("conf", rhai::Map::new());

    c.bench_function("breakdown_scope_only", |b| {
        b.iter(|| {
            let mut scope = scope_template.clone();
            scope.set_value("line", event.original_line.clone());

            let mut event_map = rhai::Map::new();
            for (k, v) in &event.fields {
                event_map.insert(k.clone().into(), v.clone());
            }
            scope.set_value("e", event_map);

            let mut meta_map = rhai::Map::new();
            if let Some(line_num) = event.line_num {
                meta_map.insert("line_num".into(), Dynamic::from(line_num as i64));
            }
            if let Some(ref filename) = event.filename {
                meta_map.insert("filename".into(), Dynamic::from(filename.clone()));
            }
            meta_map.insert("line".into(), Dynamic::from(event.original_line.clone()));
            scope.set_value("meta", meta_map);

            black_box(scope);
        });
    });
}

fn bench_breakdown_eval_only(c: &mut Criterion) {
    let engine = Engine::new();
    let ast = engine.compile_expression(r#"e.level == "ERROR""#).unwrap();

    // Pre-built scope (simulating if we could reuse)
    let mut scope_template = Scope::new();
    scope_template.push("line", "test line");
    let mut e_map = rhai::Map::new();
    e_map.insert("level".into(), Dynamic::from("ERROR"));
    e_map.insert("message".into(), Dynamic::from("test message"));
    e_map.insert("user_id".into(), Dynamic::from(12345_i64));
    scope_template.push("e", e_map);
    scope_template.push("meta", rhai::Map::new());
    scope_template.push("conf", rhai::Map::new());

    c.bench_function("breakdown_eval_only", |b| {
        b.iter(|| {
            // Just clone and eval - minimal scope work
            let mut scope = scope_template.clone();
            black_box(engine.eval_ast_with_scope::<bool>(&mut scope, &ast).unwrap());
        });
    });
}

// =============================================================================
// Dynamic cloning overhead
// =============================================================================

fn bench_dynamic_clone_string(c: &mut Criterion) {
    let d = Dynamic::from("This is a typical log message content here");
    c.bench_function("dynamic_clone_string", |b| {
        b.iter(|| {
            black_box(d.clone());
        });
    });
}

fn bench_dynamic_clone_int(c: &mut Criterion) {
    let d = Dynamic::from(12345_i64);
    c.bench_function("dynamic_clone_int", |b| {
        b.iter(|| {
            black_box(d.clone());
        });
    });
}

fn bench_dynamic_clone_map(c: &mut Criterion) {
    let mut map = rhai::Map::new();
    map.insert("level".into(), Dynamic::from("INFO"));
    map.insert("message".into(), Dynamic::from("test message here"));
    map.insert("user_id".into(), Dynamic::from(12345_i64));
    map.insert("session".into(), Dynamic::from("abc-123"));
    let d = Dynamic::from(map);
    c.bench_function("dynamic_clone_map", |b| {
        b.iter(|| {
            black_box(d.clone());
        });
    });
}

// Group benchmarks
criterion_group!(
    startup_benches,
    bench_engine_new_raw,
    bench_rhai_engine_new,
    bench_function_registration,
);

criterion_group!(
    compilation_benches,
    bench_compile_simple_filter,
    bench_compile_complex_filter,
    bench_compile_exec_script,
);

criterion_group!(
    scope_benches,
    bench_scope_new,
    bench_scope_clone_empty,
    bench_scope_clone_template,
    bench_scope_set_value_string,
    bench_scope_set_value_map,
);

criterion_group!(
    map_building_benches,
    bench_build_event_map_small,
    bench_build_event_map_large,
    bench_build_meta_map,
    bench_full_scope_creation_small,
    bench_full_scope_creation_large,
);

criterion_group!(
    eval_benches,
    bench_eval_simple_filter,
    bench_eval_complex_filter,
    bench_eval_with_string_method,
);

criterion_group!(
    e2e_benches,
    bench_e2e_filter_simple,
    bench_e2e_filter_with_kelora_engine,
);

criterion_group!(
    breakdown_benches,
    bench_breakdown_scope_only,
    bench_breakdown_eval_only,
);

criterion_group!(
    dynamic_benches,
    bench_dynamic_clone_string,
    bench_dynamic_clone_int,
    bench_dynamic_clone_map,
);

criterion_main!(
    startup_benches,
    compilation_benches,
    scope_benches,
    map_building_benches,
    eval_benches,
    e2e_benches,
    breakdown_benches,
    dynamic_benches
);
