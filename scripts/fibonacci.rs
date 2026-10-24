//! This script calculates the n-th Fibonacci number using a really dumb algorithm
//! to test the speed of the scripting engine.

use std::time::Instant;

const TARGET: i64 = 28;
const REPEAT: i64 = 5;
const ANSWER: i64 = 317_811;

fn fib(n: i64) -> i64 {
    if n < 2 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

println!("Running Fibonacci({}) x {} times...", TARGET, REPEAT);
println!("Ready... Go!");

let mut result = 0;
let now = Instant::now();

for _ in 0..REPEAT {
    result = fib(TARGET);
}

println!("Finished. Run time = {:?}.", now.elapsed());

println!("Fibonacci number #{} = {result}", TARGET);

if result != ANSWER {
    println!("The answer is WRONG! Should be {}!", ANSWER);
}
