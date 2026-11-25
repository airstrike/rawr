// rawr
// A crate's generic helper in a private module its root globs, as iced's
// `container(..)` is `widget::helpers`'s, returning the crate's generic
// type at the program's arguments: `framed` frames a crate label and the
// program's own `Shown` type, and a frame of a frame, through `dyn Shown`.

use events::framed;
use events::framed::{shown_all, shown_one, Label, Meta, Shown};

/// A mood the program shows.
struct Mood(&'static str);

impl Meta for Mood {}

impl Shown for Mood {
    fn shown(&self) -> String {
        format!("~{}~", self.0)
    }
}

fn main() {
    let items: Vec<Box<dyn Shown>> = vec![
        Box::new(framed(Label("calm".into()))),
        Box::new(framed(framed(Mood("deep")))),
        Box::new(framed(framed(framed(Weather(3))))),
    ];
    println!("{}", shown_all(&items));
    println!("{}", shown_one(&framed(Mood("lent")).edge('*')));
}

/// A new program type, framed three deep: a nesting the runner never saw.
struct Weather(u8);

impl Meta for Weather {}

impl Shown for Weather {
    fn shown(&self) -> String {
        format!("{} clouds", self.0)
    }
}
