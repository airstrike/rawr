//! Native Rust: hands another crate's updates to the marked `home`.
pub mod workspace;

fn main() {
    let mut updates = shapes::sample();
    let second = updates.pop().unwrap();
    let first = updates.pop().unwrap();
    println!("{}", workspace::screen::home::summary(first, second));
    println!("{}", workspace::count(&shapes::sample()));
}
