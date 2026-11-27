// rawr
// A crate's generic type whose field is its palette's associated type, as
// iced's `Container<'a, W, Theme>` holds `Theme::Class<'a>`: `Painted` of a
// crate label and of the program's own `Shown` type, given a class and
// given another through a bound on the class, through `dyn Shown`.

use events::framed::{shown_all, shown_one, Ink, Label, Meta, Painted, Shown};

/// A mood the program shows.
struct Mood(&'static str);

impl Meta for Mood {}

impl Shown for Mood {
    fn shown(&self) -> String {
        format!("~{}~", self.0)
    }
}

fn plain<'a>() -> Box<dyn Fn(&Ink) -> char + 'a> {
    Box::new(|_| '|')
}

fn main() {
    let items: Vec<Box<dyn Shown>> = vec![
        Box::new(Painted::new(Label("calm".into()), Ink, plain())),
        Box::new(Painted::new(Mood("bright"), Ink, plain()).class(|_| '#')),
    ];
    println!("{}", shown_all(&items));
    let lent = Painted::new(Mood("lent"), Ink, plain()).class(|_| '*');
    println!("{}", shown_one(&lent));
}
