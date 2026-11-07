// rawr
use natives::random;
use natives::weighted::{Distribution, WeightedIndex};

fn main() {
    // rustc fixes `X` only through `I::Item: SampleBorrow<X>`: the array's
    // items are `{integer}`, and so `X` is `i32`.
    let weights = [10, 4, 2, 1];
    let distribution = WeightedIndex::new(weights).expect("Create distribution");
    let mut rng = random::rng();
    let mut picks = Vec::new();
    for _ in 0..6 {
        picks.push(distribution.sample(&mut rng));
    }
    println!("{} {picks:?}", distribution.kind());
}
