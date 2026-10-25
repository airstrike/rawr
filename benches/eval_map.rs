#![feature(test)]

///! Test evaluating expressions
extern crate test;

use rawr::{Engine, OptimizationLevel};
use test::Bencher;

#[bench]
fn bench_eval_map_small_get(bench: &mut Bencher) {
    let script =
        r#"use std::collections::HashMap; let mut x = HashMap::new(); x.insert("a", 1); x["a"]"#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_map_small_set(bench: &mut Bencher) {
    let script = r#"use std::collections::HashMap; let mut x = HashMap::new(); x.insert("a", 1); x.insert("a", 42);"#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_map_large_get(bench: &mut Bencher) {
    let script = r#"use std::collections::HashMap;
                            let mut inner = HashMap::new();
                            inner.insert("hey".to_string(), "jude".to_string());
                            let mut x = HashMap::new();
                            x.insert("a".to_string(), vec![HashMap::new()]);
                            x.insert("e".to_string(), vec![HashMap::new(), inner.clone()]);
                            x["e"][1]["hey"].clone()
    "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_map_large_set(bench: &mut Bencher) {
    let script = r#"use std::collections::HashMap;
                            let mut inner = HashMap::new();
                            inner.insert("hey".to_string(), "jude".to_string());
                            let mut x = HashMap::new();
                            x.insert("a".to_string(), vec![HashMap::new()]);
                            x.insert("e".to_string(), vec![HashMap::new(), inner.clone()]);
                            if let Some(v) = x.get_mut("e") {
                                v[1].insert("hey".to_string(), "42".to_string());
                            }
    "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}
