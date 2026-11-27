// rawr
// A crate impl's generic body calls its parameter's bound method,
// `self.item.width()` with `S: Diffed<M>`, which the crate's `Label` also
// has as an inherent builder `width(self, usize)`: rustc reads the body
// generically, so at `S = Label` it is still `Diffed`'s, as iced's
// `Container::diff` reads `self.content.size()` of a `Text`.

use events::framed::Label;
use events::tinted::{self, Diffed};

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }

    fn width(&self) -> usize {
        4
    }
}

/// The columns `item` takes at the program's `Note`.
fn columns(item: &impl Diffed<Note>) -> usize {
    item.width()
}

fn main() {
    println!("{}", columns(&tinted::tallied(Label("calm".into()))));
    println!(
        "{}",
        columns(&tinted::tallied(tinted::tallied(Label("deep".into()))))
    );
    println!("{}", columns(&tinted::tallied(Note)));
    println!("[{}]", Label("x".into()).width(3).0);
}
