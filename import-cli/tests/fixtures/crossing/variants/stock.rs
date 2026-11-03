// rawr
use events::shelf::{self, Item};
use events::stock::{self, Named, Stocked};

fn main() {
    let mut items = vec![Item::new("first"), Item::new("second"), Item::new("third")];
    let stock = stock::stock(&items);
    // A supertrait's method.
    println!("label {}", stock.label());
    // A generic method at two types.
    println!("tag {} {}", stock.tag("n"), stock.tag(7));
    // The associated type, and the associated const through a method.
    println!("first {} limit {}", stock.first(), stock.limit());
    // A method that gives `Self`: a second result over the same items,
    // which outlives the first.
    let again = stock.again();
    println!("again {} {}", again.label(), again.first());
    drop(stock);
    let still = again.again();
    println!("still {}", still.label());
    drop(still);
    println!("names {}", shelf::names(&items));
    drop(again);
    // Neither result borrows the items now.
    items.push(Item::new("fourth"));
    println!("pushed {}", shelf::names(&items));
}
