// rawr
// Flip coins and roll dice with rand. Rawr imports rand from this crate's
// Cargo.toml, with no rawr.toml.
use rand::Rng;

fn main() {
    let mut rng = rand::rng();
    let mut heads = 0;
    let flips = 1000;
    for _ in 0..flips {
        if rand::random_bool(0.5) {
            heads += 1;
        }
    }
    println!("{} flips", flips);
    println!("fair: {}", heads > 400 && heads < 600);

    // A method of the `Rng` trait, which rand implements for every
    // random number generator.
    let n: u32 = rng.random_range(1..=6);
    println!("die in range: {}", n >= 1 && n <= 6);
    println!("coin: {}", rng.random_bool(1.0));

    // Generic functions, at the types the code names or infers.
    let die: u32 = rand::random_range(1..=6);
    println!("free die in range: {}", die >= 1 && die <= 6);
    let x: f64 = rand::random();
    println!("unit interval: {}", x >= 0.0 && x < 1.0);
    let byte = rand::random::<u8>();
    println!("byte: {}", u32::from(byte) <= 255);
}
