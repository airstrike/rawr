#![feature(test)]

///! Test parsing expressions
extern crate test;

use rawr::{Engine, OptimizationLevel, Scope, INT};
use test::Bencher;

#[bench]
fn bench_parse_single(bench: &mut Bencher) {
    let script = "1";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    bench.iter(|| engine.compile_expression(script).unwrap());
}

#[bench]
fn bench_parse_simple(bench: &mut Bencher) {
    let script = "(requests_made * requests_succeeded / 100) >= 90";

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let mut scope = Scope::new();
    scope.push("requests_made", 99 as INT);
    scope.push("requests_succeeded", 90 as INT);

    bench.iter(|| engine.compile_expression_with_scope(&scope, script).unwrap());
}

#[bench]
fn bench_parse_full(bench: &mut Bencher) {
    let script = r#"
            2 > 1 &&
            "something" != "nothing" ||
            "2014-01-20" < "Wed Jul  8 23:07:35 MDT 2015" &&
            vec![1, 2, 3].len() <= "name".len() &&
            5 + 1000 / 2 > (80 * 100 % 2)
        "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    bench.iter(|| engine.compile_expression(script).unwrap());
}

#[bench]
fn bench_parse_array(bench: &mut Bencher) {
    let script = r#"(1, 234.789, "hello", false, [9, 8, 7])"#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    bench.iter(|| engine.compile_expression(script).unwrap());
}

#[bench]
fn bench_parse_map(bench: &mut Bencher) {
    let script = r#"std::collections::HashMap::from([("a", 1), ("b", 42), ("c", 3), ("dc%$& ", 4), ("x", 5), ("y", 6)])"#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    bench.iter(|| engine.compile_expression(script).unwrap());
}

#[bench]
fn bench_parse_primes(bench: &mut Bencher) {
    let script = r#"
            // This script uses the Sieve of Eratosthenes to calculate prime numbers.
            use std::time::Instant;

            let now = Instant::now();

            const MAX_NUMBER_TO_CHECK: usize = 10_000;     // 1229 primes <= 10000

            let mut prime_mask = vec![true; MAX_NUMBER_TO_CHECK];

            prime_mask[0] = false;
            prime_mask[1] = false;

            let mut total_primes_found = 0;

            for p in 2..MAX_NUMBER_TO_CHECK {
                if prime_mask[p] {
                    println!("{p}");

                    total_primes_found += 1;
                    let mut i = 2 * p;

                    while i < MAX_NUMBER_TO_CHECK {
                        prime_mask[i] = false;
                        i += p;
                    }
                }
            }

            println!("Total {total_primes_found} primes.");
            println!("Run time = {:?}.", now.elapsed());
        "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    bench.iter(|| engine.compile(script).unwrap());
}

#[bench]
fn bench_parse_optimize_simple(bench: &mut Bencher) {
    let script = r#"
            2 > 1 &&
            "something" != "nothing" ||
            "2014-01-20" < "Wed Jul  8 23:07:35 MDT 2015" &&
            vec![1, 2, 3].len() <= "name".len() &&
            5 + 1000 / 2 > (80 * 100 % 2)
        "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::Simple);

    bench.iter(|| engine.compile_expression(script).unwrap());
}

#[bench]
fn bench_parse_optimize_full(bench: &mut Bencher) {
    let script = r#"
            2 > 1 &&
            "something" != "nothing" ||
            "2014-01-20" < "Wed Jul  8 23:07:35 MDT 2015" &&
            vec![1, 2, 3].len() <= "name".len() &&
            5 + 1000 / 2 > (80 * 100 % 2)
        "#;

    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::Full);

    bench.iter(|| engine.compile_expression(script).unwrap());
}
