// rawr
// A crate function over `I: Iterator` takes an iterator, not an array:
// rustc's E0277, as `cargo build` gives it.

fn main() {
    println!("{}", events::paired::length(vec![1, 2, 3]));
}
