// rawr
// A crate generic type's field of its palette's class, as iced's
// `Container<'a, W, Theme>` holds `Theme::Class<'a>`, a boxed closure:
// the class native code makes through `P::default()`, as `Container::new`
// takes `Theme::default()`, and the program's own closure, each lent to
// the crate's concrete `paint` as `theme.style(&self.class)` lends it.
use events::framed::{Ink, Label, Painted};

fn main() {
    let plain = Painted::plain(Label("calm".into()), Ink);
    println!("{}", plain.painted());
    let classed = Painted::plain(Label("deep".into()), Ink).class(|_| '#');
    println!("{}", classed.painted());
}
