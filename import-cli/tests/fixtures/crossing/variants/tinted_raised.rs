// rawr
// A program's own call of its impl's method that calls a native method
// whose output keeps its `&mut` receiver and a `&mut` input beside a value
// it reads for the call alone, as a program's `Widget::overlay` asks its
// content for its overlay: Rawr runs such a body only where native code
// calls it, in the region that lends what the output keeps, and refuses
// the program's own call at check, where rustc compiles it.

use events::visits::{Lift, Raised};
use events::walks::{Lifted, Node};

struct Wrap<R>(R);

impl<R: Raised> Raised for Wrap<R> {
    fn raised<'b>(&'b mut self, node: &'b mut Node, depth: &usize) -> Vec<Lifted<'b>> {
        self.0.raised(node, depth)
    }
}

fn main() {
    let mut wrap = Wrap(Lift("up".into()));
    let mut node = Node::new(Vec::new());
    println!("{}", wrap.raised(&mut node, &1).len());
}
