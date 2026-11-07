// rawr
use natives::random::{self, Weighted};

fn main() {
    let weights = Weighted::new(10, 4, 2);
    let mut rng = random::rng();
    let mut picks = Vec::new();
    for _ in 0..6 {
        picks.push(weights.sample(&mut rng));
        picks.push(weights.sample_thread(&mut rng));
    }
    println!("{picks:?} {}", rng.below(10));
}
