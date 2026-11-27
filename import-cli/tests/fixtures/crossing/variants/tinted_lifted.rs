// rawr
// A crate trait's provided method whose signature can't cross, as iced's
// `Widget::overlay`, whose output borrows the widget and its tree. Where
// the impl rustc selects leaves the method to the trait, the program runs
// the trait's provided body from the trait's module source. The body calls
// each item's own `label`: natively for the crate's `Mark`, from source for
// the crate's `Label`, and the program's own for its `Note`.

use events::framed::Label;
use events::tinted::{self, Diffed};
use events::visits::Mark;
use events::walks::Node;

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// How many nodes `item` lifts out of a new node, and the node's marks.
fn lifted(item: &mut impl Diffed<Note>) -> (usize, Vec<String>) {
    let mut node = Node::default();
    let count = item.lifted(&mut node).len();
    (count, node.listing())
}

fn main() {
    let mut calm = tinted::tallied(Label("calm".into()));
    println!("{:?}", lifted(&mut calm));
    let mut leaf = tinted::tallied(Mark("leaf".into()));
    println!("{:?}", lifted(&mut leaf));
    let mut note = tinted::tallied(tinted::tallied(Note));
    println!("{:?}", lifted(&mut note));
}
