// rawr
// A crate's generic type at the program's arguments, through the crate's
// trait object: `Framed<S>` of a crate label and of the program's own
// `Shown` type, boxed as `dyn Shown` for the crate's concrete `shown_all`.

use events::framed::{self, Label, Meta, Shown};
use events::held::Held;

/// A mood the program shows.
struct Mood(&'static str);

impl Meta for Mood {}

impl Shown for Mood {
    fn shown(&self) -> String {
        format!("~{}~", self.0)
    }
}

/// A cell that shows nothing, which the crate leaves out.
struct Blank;

impl Meta for Blank {
    fn is_void(&self) -> bool {
        true
    }
}

impl Shown for Blank {
    fn shown(&self) -> String {
        "blank".into()
    }
}

fn main() {
    let items: Vec<Box<dyn Shown>> = vec![
        Box::new(framed::framed(Label("calm".into()))),
        Box::new(Mood("plain")),
        Box::new(Blank),
        Box::new(framed::framed(Blank)),
        Box::new(framed::framed(Mood("bright")).edge('#')),
    ];
    println!("{}", framed::shown_all(&items));
    let lent: &dyn Shown = &Mood("lent");
    println!("{}", framed::shown_one(lent));
    let lent: &dyn Shown = &framed::framed(Mood("lent")).edge('*');
    println!("{}", framed::shown_one(lent));
    println!(
        "{}",
        framed::shown_one(&framed::framed(Mood("given")).edge('+'))
    );
    println!("{}", Held::new(framed::framed(Mood("held"))).shown());
}
