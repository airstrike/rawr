// rawr
use events::shelf::{self, Item, Show};

fn main() {
    let mut items = vec![Item::new("first")];
    let view = shelf::view(&items);
    items.push(Item::new("second"));
    println!("view {}", view.show());
}
