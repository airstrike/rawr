// rawr
// Flip coins with rand. Rawr imports rand from this crate's Cargo.toml,
// with no rawr.toml.

fn main() {
    let _rng = rand::rng();
    let mut heads = 0;
    let flips = 1000;
    for _ in 0..flips {
        if rand::random_bool(0.5) {
            heads += 1;
        }
    }
    println!("{} flips", flips);
    println!("fair: {}", heads > 400 && heads < 600);
}
