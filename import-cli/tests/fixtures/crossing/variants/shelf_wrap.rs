// rawr
use events::shelf::{self, Item, Show};

fn main() {
    let mut items = vec![Item::new("first"), Item::new("second")];
    let wrapped = shelf::wrap(shelf::view(&items));
    println!("wrapped {}", wrapped.show());
    // A wrapped result wrapped again.
    let twice = shelf::wrap(wrapped);
    println!("twice {}", twice.show());
    println!("names {}", shelf::names(&items));
    drop(twice);
    items.push(Item::new("third"));
    // A result moved out of a binding.
    let view = shelf::view(&items);
    let wrapped = shelf::wrap(view);
    println!("later {}", wrapped.show());
    drop(wrapped);
    items.push(Item::new("fourth"));
    {
        let scoped = shelf::wrap(shelf::pick(&items));
        println!("scoped {} {}", scoped.show(), scoped.show());
    }
    items.push(Item::new("fifth"));
    println!("counted {}", shelf::names(&items));
}
