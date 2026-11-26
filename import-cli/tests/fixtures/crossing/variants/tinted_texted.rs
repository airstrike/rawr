// rawr
// A crate's generic impl for `&str` of its trait, which the crate compiles,
// at the program's string: the opened `Tallied<S>` impl runs from source at
// `S = &str` and calls the string's `label` and `width`, as iced's
// `Container` runs from source over `container("")` and calls the string's
// `Widget::tag`.

use events::tinted::{self, Diffed};

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

fn main() {
    println!("{:?}", shown(tinted::tallied("leaf")));
}
