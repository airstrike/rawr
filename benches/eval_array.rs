#![feature(test)]

///! Test evaluating expressions
extern crate test;

use rawr::{Engine, OptimizationLevel};
use test::Bencher;

#[bench]
fn bench_eval_array_small_get(bench: &mut Bencher) {
    let script = "let x = vec![1, 2, 3, 4, 5]; x[3]";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_array_small_set(bench: &mut Bencher) {
    let script = "let mut x = vec![1, 2, 3, 4, 5]; x[3] = 42;";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_array_large_get(bench: &mut Bencher) {
    let script = r#"let x = vec![
                                vec![ vec!["hey".to_string()], vec!["deeply".to_string(), "nested".to_string()] ],
                                vec![ vec!["jude".to_string()] ]
                            ];
                            x[0][1][1].clone()
    "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_array_large_set(bench: &mut Bencher) {
    let script = r#"let mut x = vec![
                                vec![ vec!["hey".to_string()], vec!["deeply".to_string(), "nested".to_string()] ],
                                vec![ vec!["jude".to_string()] ]
                            ];
                            x[0][1][1] = "42".to_string();
    "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_eval_array_loop(bench: &mut Bencher) {
    let script = "
            let mut list = Vec::new();
            
            for i in 0..10_000 {
                list.push(i);
            }

            let mut sum = 0;

            for i in list {
                sum += i;
            }
        ";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}
