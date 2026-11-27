// rawr
// A crate's generic code run from source reads native code's `Vec<Node>`
// as the native vector it is, as iced's `diff_children_custom` reads a
// `Tree`'s children: `len`, `truncate`, `extend` with new native nodes,
// and `iter_mut`, whose each node is marked where it is.

use events::tinted::{self, Diffed};
use events::walks::Node;

struct Note(&'static str);

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        self.0.into()
    }
}

fn main() {
    let mut node = Node::new(vec![Node::default(), Node::default(), Node::default()]);
    let mut two = [Note("a"), Note("b")];
    tinted::regrow(&mut node, &mut two);
    println!("{:?}", node.listing());
    let mut four = [Note("c"), Note("d"), Note("e"), Note("f")];
    tinted::regrow(&mut node, &mut four);
    println!("{:?}", node.listing());
}
