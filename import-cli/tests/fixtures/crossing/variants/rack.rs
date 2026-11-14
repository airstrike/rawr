// rawr
use events::rack::{self, Rack};
use events::shelf::{self, Item};

fn main() {
    let mut items = vec![Item::new("first"), Item::new("second")];
    let held = rack::rack(&items);
    println!("count {}", held.count());
    println!("tally {}", rack::tally(held));
    println!("again {}", rack::tally(rack::rack(&items)));
    items.push(Item::new("third"));
    println!("names {}", shelf::names(&items));
}
