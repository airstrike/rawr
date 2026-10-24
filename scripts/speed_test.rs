//! This script runs 1 million iterations to test the speed of the scripting engine.

use std::time::Instant;

let now = Instant::now();
let mut x = 1_000_000;

println!("Ready... Go!");

while x > 0 {
    x -= 1;
}

println!("Finished. Run time = {:?}.", now.elapsed());
