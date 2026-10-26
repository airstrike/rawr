struct View<'a> {
    name: &'a String,
}

fn main() {
    let mut name = String::from("a");
    let view = View { name: &name };
    let mut __o = Some(&name);
    __see(&__o);
    __o = None;
    name = String::from("b");
    __see(&__o);
    println!("{} {name}", view.name);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);
