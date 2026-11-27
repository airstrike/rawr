// rawr
// Native code asks a program's value for what it lifts out of a node, as
// iced's `UserInterface` asks the root widget for its overlay: the value
// crosses as the trait's stand-in, whose `lifted` runs the program's
// impl. That impl gives back what a native item's own `lifted` lifted,
// borrowed from the item and the node native code lent, as iced's
// `Container::overlay` gives its content's `overlay::Element<'b, ..>`.

use events::tinted::{self, Diffed};
use events::visits::Lift;
use events::walks;

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// What native code's ask of `item` lifts at the program's `Note`.
fn lifted(item: impl Diffed<Note>) -> (usize, Vec<String>) {
    walks::lifted_count(item)
}

fn main() {
    println!("{:?}", lifted(tinted::tallied(Lift("up".into()))));
    println!("{:?}", lifted(Note));
}
