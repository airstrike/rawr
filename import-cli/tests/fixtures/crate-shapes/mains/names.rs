// rawr
use natives::names::{self, import, Handle};

fn main() {
    println!("{}", names::print("once"));
    println!("{}", import::switch(false));
    let debug = Handle { this: 2 };
    println!(
        "{}",
        debug.call(3) + natives::names::Handle { this: 1 }.this
    );
}
