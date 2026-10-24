#![feature(test)]

///! Test evaluating expressions
extern crate test;

use rawr::{Engine, OptimizationLevel};
use test::Bencher;

// This script uses the Sieve of Eratosthenes to calculate prime numbers.

const SCRIPT: &str = "
    const MAX_NUMBER_TO_CHECK: usize = 1_000;     // 168 primes <= 1000

    let mut prime_mask = vec![true; MAX_NUMBER_TO_CHECK];

    prime_mask[0] = false;
    prime_mask[1] = false;

    let mut total_primes_found = 0;

    for p in 2..MAX_NUMBER_TO_CHECK {
        if prime_mask[p] {
            total_primes_found += 1;
            let mut i = 2 * p;

            while i < MAX_NUMBER_TO_CHECK {
                prime_mask[i] = false;
                i += p;
            }
        }
    }
";

#[bench]
fn bench_eval_primes(bench: &mut Bencher) {
    let mut engine = Engine::new();
    engine.set_optimization_level(OptimizationLevel::None);

    let ast = engine.compile(SCRIPT).unwrap();

    bench.iter(|| engine.run_ast(&ast).unwrap());
}
