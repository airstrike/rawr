//! This script runs for-loops with closures.

use std::time::Instant;

const MAX: i64 = 100;
// Each closure keeps its own copy of `i`, so the sum is 0² + 1² + .. + 99².
const CHECK: i64 = (MAX - 1) * MAX * (2 * MAX - 1) / 6;

println!("Ready... Go!");

let now = Instant::now();

println!("Creating {} closures...", MAX);

let mut list: Vec<Box<dyn Fn() -> i64>> = Vec::new();

// Loop over range
for i in 0..MAX {
    list.push(Box::new(move || i * i));
}

println!("Time = {:?}...", now.elapsed());
println!("Summing {} closures...", MAX);

let mut sum = 0;

// Loop over array
for f in list {
    sum += f();
}

println!("Sum = {sum} (should be {})", CHECK);
println!("Finished. Total run time = {:?}.", now.elapsed());
