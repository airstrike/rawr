// rawr
// A marked crate in a Cargo workspace. Its dependencies come from the
// workspace's [workspace.dependencies].

fn main() {
    println!("a die has {} sides", dice::sides());
    println!("seed 9 shows {}", dice::face(9));
    println!("coin: {}", rand::random_bool(1.0));
}
