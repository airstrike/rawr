// rawr
use events::shelf::{self, Item, Show};

fn main() {
    let mut items = vec![Item::new("first")];
    let view = shelf::view(&items);
    println!("view {}", view.show());
    items.push(Item::new("second"));
    println!("pushed {}", shelf::names(&items));
}
