//! This script uses the Sieve of Eratosthenes to calculate prime numbers.

use std::time::Instant;

let now = Instant::now();

const ANSWER: i64 = 78_498;
const MAX_NUMBER_TO_CHECK: usize = 1_000_000;

let mut prime_mask = vec![true; MAX_NUMBER_TO_CHECK + 1];

prime_mask[0] = false;
prime_mask[1] = false;

let mut total_primes_found = 0;

for p in 2..=MAX_NUMBER_TO_CHECK {
    if !prime_mask[p] { continue; }

    //println!("{p}");

    total_primes_found += 1;

    for i in (2 * p..=MAX_NUMBER_TO_CHECK).step_by(p) {
        prime_mask[i] = false;
    }
}

println!("Total {total_primes_found} primes <= {}", MAX_NUMBER_TO_CHECK);
println!("Run time = {:?}.", now.elapsed());

if total_primes_found != ANSWER {
    println!("The answer is WRONG! Should be {}!", ANSWER);
}
