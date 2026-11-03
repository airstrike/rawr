// rawr
use events::shelf::{self, Item, Show};

fn main() {
    let mut items = vec![Item::new("first")];
    let wrapped = shelf::wrap(shelf::view(&items));
    items.push(Item::new("second"));
    println!("wrapped {}", wrapped.show());
}
