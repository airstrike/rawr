// rawr
//! A marked file a universe runner loads as `?play` loads a snippet: in a
//! `rawr::universe::Universe` of the bindings the runner's build generated
//! for this crate's dependencies (#1387).
use natives::names::{self, Handle};

fn main() {
    println!("{}", names::print("by the universe"));
    println!("{}", Handle { this: 4 }.call(5));
}
