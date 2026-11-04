// rawr
// A crate's `-> !` functions: a call of one is a value of whatever type
// its place takes, in an `if` or `match` arm and a `let .. else`, and the
// program ends where `finish` ends the process.

use events::diverging::{fail, finish};

fn pick(n: i32) -> i32 {
    if n > 0 {
        n
    } else {
        fail("negative")
    }
}

fn first(values: &[u32]) -> u32 {
    let Some(value) = values.first() else {
        fail("empty")
    };
    *value
}

fn main() {
    println!("{}", pick(3));
    let x: u32 = match Some(2) {
        Some(v) => v,
        None => fail("none"),
    };
    println!("{x} {}", first(&[5, 6]));
    let code: i32 = if x == 2 { finish("finishing", 0) } else { 1 };
    println!("unreached {code}");
}
