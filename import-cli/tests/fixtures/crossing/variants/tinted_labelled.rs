// rawr
// A crate type's generic method whose bound is a crate trait the module
// imports by a single-name `use`, at a crate label and at the program's
// own `Shown` type.

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
    let style = tinted::plain();
    println!("{}", style.labelled(&Label("calm".into())));
    println!("{}", style.labelled(&Loud("bright")));
}
