//! Native Rust: the app's message enum, an alias, a constant, a helper and
//! a re-export live in the crate root, and the marked module `badge` names
//! them through `crate::` and `super::`, as rustc lets a descendant do.
mod badge;
mod page;

use page::Kind;

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Page(usize, page::Message),
    Noop,
}

type Event = page::Message;

const SCALE: usize = 10;

fn helper(index: usize) -> usize {
    index * SCALE
}

fn main() {
    for index in 0..3 {
        println!("{}", badge::message(index));
    }
    for kind in [Kind::Narrow, Kind::Wide] {
        println!("{}", badge::describe(kind == Kind::Wide));
    }
}
