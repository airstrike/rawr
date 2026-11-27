// rawr
// A crate's generic type whose method gives its field to a crate type's
// generic method as `std::slice::from_mut`, which hands a crate field's
// `&mut Vec` on to a generic function, as iced's `Container::diff` calls
// `Tree::diff_children`.

use events::framed::{Label, Meta, Shown};
use events::tinted;

struct Loud(&'static str);

impl Meta for Loud {}

impl Shown for Loud {
    fn shown(&self) -> String {
        self.0.to_uppercase()
    }
}

fn main() {
    let mut calm = tinted::tallied(Label("calm".into()));
    println!("{:?}", calm.diffed());
    let mut loud = tinted::tallied(Loud("bright"));
    println!("{:?}", loud.diffed());
    let mut tally = tinted::Tally::new();
    tally.labels.push("old".into());
    tally.labels.push("older".into());
    let mut fresh = [Loud("new")];
    tally.diff_children(&mut fresh);
    println!("{:?}", tally.labels);
}
