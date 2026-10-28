// rawr
// A program that shares a dependency with rawr's runtime, at fewer features
// than rawr turns on: its runner links the shared runtime, with a lockfile
// cargo's `--locked` build accepts.
use ahash::RandomState;

fn main() {
    let state = RandomState::with_seeds(1, 2, 3, 4);
    drop(state);
    println!("seeded");
}
