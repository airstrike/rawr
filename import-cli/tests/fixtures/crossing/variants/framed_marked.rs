// rawr
// A crate helper whose `where` clause names a generic alias through a
// module that re-exports it from another crate by a glob, as iced's
// `helpers::iced` writes `From<text::StyleFn<'a, Theme>>` and iced_widget's
// `text` is `pub use crate::core::widget::text::*;`.

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
    println!("{}", shown_one(&events::marked(Mood("marked"), Ink)));
}
