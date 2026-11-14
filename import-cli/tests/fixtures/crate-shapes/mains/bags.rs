// rawr
// A `for` loops over a crate's value by its `IntoIterator` impl, as
// integration's `for message in messages` loops over iced's `shell::Bus`,
// and a `&mut` of a crate's value made for the call fills its input.
use natives::bags::{counted, Bag};

fn main() {
    let mut bag = Bag::new();
    bag.push(2u32);
    bag.push(3u32);
    for item in bag {
        println!("{item}");
    }
    println!("{}", counted(&mut Bag::new(), 4));
}
