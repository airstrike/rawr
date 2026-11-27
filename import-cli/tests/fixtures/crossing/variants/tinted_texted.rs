// rawr
// A crate's generic impl for `&str` of its trait, which the crate compiles,
// at the program's string: the opened `Tallied<S>` impl runs from source at
// `S = &str` and calls the string's `label` and `width`, as iced's
// `Container` runs from source over `container("")` and calls the string's
// `Widget::tag`. Native code's ask for what it lifts runs the trait's
// provided `lifted`, which the impl for `&str` leaves to the trait, from
// the trait's source, as iced's `overlay` of a string does.

use events::tinted::{self, Diffed};
use events::walks;

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// The label and width of `item` at the program's `Note`.
fn shown(mut item: impl Diffed<Note>) -> (String, usize) {
    (item.label(), item.width())
}

/// What native code's ask of `item` lifts at the program's `Note`.
fn lifted(item: impl Diffed<Note>) -> (usize, Vec<String>) {
    walks::lifted_count(item)
}

fn main() {
    println!("{:?}", shown(tinted::tallied("leaf")));
    println!("{:?}", lifted(tinted::tallied("leaf")));
}
