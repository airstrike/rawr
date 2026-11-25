// rawr
// A crate's generic code runs from source at the program's `Note` over a
// native `Mark`, whose impl is at any `M`, as iced's `Container` and
// `Tree::diff` run over a native `Text` at the program's `Message`. Each
// native call takes the program's `Note` from the generic body's bound,
// where none of the call's inputs names it. `Self::of(item)` in
// `Tally::rediff` is the crate's generic `of` from source at `Note`, as
// iced's `Self::new(new)` in `Tree::diff` is, and so is `Tally::of(item)`
// in a generic function, as iced's `Tree::new(widget)` in
// `diff_children` is. `self.item.seen(..)` in `Tallied::seen` is `Mark`'s
// native `seen` at `Note`, as iced's `self.content.draw(..)` is `Text`'s.

use events::tinted::{self, Diffed, Tally};
use events::visits::Mark;
use events::walks;

struct Note;

impl Diffed<Note> for Note {
    fn label(&mut self) -> String {
        "note".into()
    }
}

/// The tally `Tally::rediff` makes of `item` at the program's `Note`.
fn rediffed(item: &mut impl Diffed<Note>) -> Vec<String> {
    let mut tally = Tally::new();
    tally.rediff(item);
    tally.labels
}

/// What native code's walk of `item` gives at the program's `Note`.
fn walked(item: impl Diffed<Note>) -> Vec<String> {
    walks::walked(item)
}

fn main() {
    let mut leaf = Mark("leaf".into());
    println!("{:?}", rediffed(&mut leaf));
    let mut note = Note;
    println!("{:?}", rediffed(&mut note));
    println!("{:?}", tinted::tally_of(&mut note).labels);
    println!("{:?}", walked(tinted::tallied(Mark("leaf".into()))));
}
