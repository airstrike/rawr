// rawr
// A crate helper whose `where` clause names its bound's trait through a
// qualified projection by the trait's path, and the opened module's
// generic alias by its module, as iced's `helpers::iced` writes
// `<Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a,
// Theme>>`.

use events::framed::{shown_one, Ink, Meta, Shown};

/// A mood the program shows.
struct Mood(&'static str);

impl Meta for Mood {}

impl Shown for Mood {
    fn shown(&self) -> String {
        format!("~{}~", self.0)
    }
}

fn main() {
    println!("{}", shown_one(&events::badge(Mood("badged"), Ink)));
}
