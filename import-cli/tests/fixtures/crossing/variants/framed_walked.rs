// rawr
// A program's value boxed as a crate trait's object the runner has no
// stand-in for, as iced's `Widget` with its `operate(&mut dyn Operation)`:
// rawr refuses it at check, before anything runs.
use events::framed::Label;
use events::held::{walked_all, Visitor, Walked};

/// A mood the program walks.
struct Mood(&'static str);

impl Walked for Mood {
    fn walk(&self, _visitor: &mut dyn Visitor) {
        let _ = self.0;
    }
}

fn main() {
    let items: Vec<Box<dyn Walked>> = vec![Box::new(Label("calm".into())), Box::new(Mood("deep"))];
    println!("{}", walked_all(&items));
}
