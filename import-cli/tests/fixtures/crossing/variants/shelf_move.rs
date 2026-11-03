// rawr
use events::shelf::{self, Item, Show};

fn main() {
    let items = vec![Item::new("first")];
    let view = shelf::view(&items);
    let moved = items;
    println!("view {} {}", view.show(), moved.len());
}
