#![feature(test)]

///! Test 1,000 iterations
extern crate test;

use rawr::{Engine, OptimizationLevel, INT};
use test::Bencher;

#[bench]
fn bench_iterations_1000(bench: &mut Bencher) {
    let script = "
            let mut x = 1_000;

            while x > 0 {
                x -= 1;
            }
        ";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_iterations_fibonacci(bench: &mut Bencher) {
    let script = "
        fn fibonacci(n: i64) -> i64 {
            if n < 2 {
                n
            } else {
                fibonacci(n-1) + fibonacci(n-2)
            }
        }

        fibonacci(20)
    ";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.eval_ast::<INT>(&ast).unwrap());
}

#[bench]
fn bench_iterations_array(bench: &mut Bencher) {
    let script = "
            let mut x = vec![0; 1000];
            for i in 0..1000 { x[i] = i as i64 % 256; }
        ";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}

#[bench]
fn bench_iterations_blob(bench: &mut Bencher) {
    let script = "
            let mut x = vec![0_u8; 1000];
            for i in 0..1000 { x[i] = (i % 256) as u8; }
        ";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(script).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}
