// rawr
// A local `String` borrowed where only a `&'static str` goes:
// `Named::with_name` takes `&'static str`, so rustc reports E0597 at the
// borrow (#667).

use events::controls::Named;

fn main() {
    let name = String::from("Georgia");
    println!("{}", Named::with_name(&name).len());
}
