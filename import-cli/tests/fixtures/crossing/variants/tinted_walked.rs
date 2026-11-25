// rawr
// Native code lends a program's impl a node it owns and a spot, as iced's
// `UserInterface` lends a widget's `operate` its `&mut Tree` and a
// `Layout`: the impl walks the node's children through the crate's
// `Spot::iter_mut` and `Spot::iter`, whose items borrow the children, and
// passes each child's node on to the item's impl, which runs from source
// too.

use events::framed::Label;
use events::tinted::{self, Diffed};
use events::walks;

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// What native code's walk of `item` gives at the program's `Note`.
fn walked(item: impl Diffed<Note>) -> Vec<String> {
    walks::walked(item)
}

fn main() {
    println!("{:?}", walked(tinted::tallied(Label("calm".into()))));
    println!(
        "{:?}",
        walked(tinted::tallied(tinted::tallied(Label("deep".into()))))
    );
}
