// rawr
use natives::form::{form, Filter};

fn main() {
    println!("{}", form(7_u32).value());
    println!("{}", form(8_u32).label("value"));
}
