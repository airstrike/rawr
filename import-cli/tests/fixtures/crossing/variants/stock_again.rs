// rawr
use events::shelf::Item;
use events::stock::{self, Named, Stocked};

fn main() {
    let mut items = vec![Item::new("first")];
    let stock = stock::stock(&items);
    let again = stock.again();
    drop(stock);
    items.push(Item::new("second"));
    println!("again {}", again.label());
}
