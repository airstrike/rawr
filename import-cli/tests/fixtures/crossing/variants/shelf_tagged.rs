// rawr
use events::shelf::{self, Item, Tagged};

fn main() {
    let items = vec![Item::new("first")];
    let tagged = shelf::tagged(&items);
    println!("tagged {}", tagged.tag("n"));
}
