// rawr
// Native code lends a program's impl a crate trait's object, `&mut dyn
// Visit`, as iced's `UserInterface::operate` lends a widget's `operate` a
// `&mut dyn Operation`: the program calls its `&mut self` methods and
// passes it on to the item's impl, which runs from source too.

use events::framed::Label;
use events::tinted::{self, Diffed};
use events::visits;

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// The labels native code's visit of `item` notes at the program's `Note`.
fn visited(item: impl Diffed<Note>) -> Vec<String> {
    visits::visited(item)
}

fn main() {
    println!("{:?}", visited(tinted::tallied(Label("calm".into()))));
    println!(
        "{:?}",
        visited(tinted::tallied(tinted::tallied(Label("deep".into()))))
    );
}
