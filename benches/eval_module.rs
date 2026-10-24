#![feature(test)]

///! Test calling script functions
extern crate test;

use rawr::{Engine, OptimizationLevel};
use test::Bencher;

#[bench]
fn bench_eval_function_call(bench: &mut Bencher) {
    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine
        .compile(
            "
                fn foo(x: i64) -> i64 { x - 1 }
                fn bar(x: i64) -> i64 { foo(x) }
                bar(41)
            ",
        )
        .unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}
