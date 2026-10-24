//! This script runs for-loops

use std::time::Instant;

const MAX: i64 = 1_000_000;

println!("Iterating an array with {} items...", MAX);

println!("Ready... Go!");

let now = Instant::now();

let mut list = Vec::new();

// Loop over range
for i in 0..MAX {
    list.push(i);
}

println!("Time = {:?}...", now.elapsed());

let mut sum = 0;

// Loop over array
for i in list {
    sum += i;
}

println!("Sum = {sum}");
println!("Finished. Total run time = {:?}.", now.elapsed());
