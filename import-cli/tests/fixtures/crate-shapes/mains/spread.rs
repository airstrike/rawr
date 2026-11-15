// rawr
use natives::spread::prelude::*;
use natives::spread::{self, Shade};

fn main() {
    let shades: Vec<Shade> = spread::shades().cloned().drive();
    println!("{shades:?}");
    let names: Vec<String> = shades.iter().map(|shade| shade.to_string()).collect();
    println!("{names:?}");
}
