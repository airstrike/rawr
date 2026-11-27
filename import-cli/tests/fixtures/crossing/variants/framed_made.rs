// rawr
// A crate's generic helper in a private module its root globs, as iced's
// `container(..)` is `widget::helpers`'s, returning the crate's generic
// type at the program's arguments: `framed` frames a crate label and the
// program's own `Shown` type, and a frame of a frame, through `dyn Shown`.

use events::framed;
use events::framed::{shown_all, shown_one, EdgeFn, Ink, Label, Meta, Shown};

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
    ];
    println!("{}", shown_all(&items));
    println!("{}", shown_one(&framed(Mood("lent")).edge('*')));
    let red = events::core::graphics::color::Color::from([1.0, 0.0, 0.0, 1.0]);
    println!("{}", shown_one(&events::reddened(Mood("red"), red)));
    // A generic alias of an opened module, imported by its name.
    let class: EdgeFn<'_, Ink> = Box::new(|_| '|');
    println!(
        "{}",
        shown_one(&events::painted(Mood("painted"), Ink, class))
    );
}
