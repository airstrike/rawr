// rawr
// `framed_types.rs` edited: `Framed` at arguments no earlier run built, a frame
// of a frame and a frame of a new program type.

use events::framed::{self, Label, Shown};

/// A mood the program shows.
struct Mood(&'static str);

impl Shown for Mood {
    fn shown(&self) -> String {
        format!("~{}~", self.0)
    }
}

/// A count the program shows.
struct Count(u32);

impl Shown for Count {
    fn shown(&self) -> String {
        format!("{} items", self.0)
    }
}

fn main() {
    let items: Vec<Box<dyn Shown>> = vec![
        Box::new(framed::framed(Label("calm".into()))),
        Box::new(framed::framed(Mood("bright")).edge('#')),
        Box::new(framed::framed(framed::framed(Label("deep".into()))).edge('[')),
        Box::new(framed::framed(Count(3))),
    ];
    println!("{}", framed::shown_all(&items));
}
