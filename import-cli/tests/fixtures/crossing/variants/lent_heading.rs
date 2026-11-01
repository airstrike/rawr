// rawr
// A local `String` borrowed where only a `&'static str` goes: `heading`
// takes `impl Heading`, which the crate implements for `&'static str`, so
// rustc reports E0597 at the borrow (#667).

use events::controls::heading;

fn main() {
    let name = String::from("Georgia");
    println!("{}", heading(name.as_str()));
}
