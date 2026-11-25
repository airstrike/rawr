// rawr
// A crate's generic impl of its trait for a concrete crate type, `impl<M>
// Diffed<M> for Label`, runs from source at the program's `Note` over
// native labels, as iced's `impl<Message, ..> Widget<Message, ..> for
// Text` runs at the program's message: through a crate method whose `M`
// only its items' bound names, and through `Tallied`'s impl, whose `M`
// the caller's bound fixes.

use events::framed::Label;
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
    let mut tally = tinted::Tally::new();
    let mut labels = [Label("calm".into()), Label("deep".into())];
    tally.diff_marked::<Note>(&mut labels);
    println!("{:?}", tally.labels);
    let mut notes = [Note];
    tally.diff_marked(&mut notes);
    println!("{:?}", tally.labels);
    let mut calm = tinted::tallied(Label("calm".into()));
    println!("{}", noted(&mut calm));
    let mut nested = tinted::tallied(tinted::tallied(Label("deep".into())));
    println!("{}", noted(&mut nested));
}
