struct View<'a> {
    name: &'a String,
}

fn main() {
    let mut name = String::from("a");
    let view = View { name: &name };
    let __k = __keep(&name);
    for __i in 0..2 {
        __see(__k);
        println!("{}", view.name);
    }
    name = String::from("b");
    println!("{name}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
