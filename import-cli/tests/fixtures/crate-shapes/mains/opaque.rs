// rawr
use natives::form::{form, Filter, Source};

fn main() {
    println!("{}", form(7_u32).value());
    println!("{}", form(8_u32).label("value"));
    let _more = form(2_u32).and(3);
    let _filter = Source(5).filter();
}
