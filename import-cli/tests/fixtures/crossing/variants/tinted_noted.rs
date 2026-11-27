// rawr
// A crate impl's parameter that only its trait names, `M` of `impl<S, M>
// Diffed<M> for Tallied<S> where S: Diffed<M>`, takes what the caller's
// own bound gives it, as rustc selects `item: &mut impl Diffed<Note>`'s
// where-clause: the nested tally diffs at `Note` through the impl's
// bound, as iced's `Container` diffs its content at its `Message`.

use events::tinted::{self, Diffed};

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// The label `item` makes as it diffs at the program's `Note`.
fn noted(item: &mut impl Diffed<Note>) -> String {
    item.label()
}

fn main() {
    let mut once = tinted::tallied(Note);
    println!("{}", noted(&mut once));
    let mut nested = tinted::tallied(tinted::tallied(Note));
    println!("{}", noted(&mut nested));
}
