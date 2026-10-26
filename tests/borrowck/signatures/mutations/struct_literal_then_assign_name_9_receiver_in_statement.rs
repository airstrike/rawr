struct View<'a> {
    name: &'a String,
}

fn main() {
    let mut name = String::from("a");
    let view = View { name: &name };
    name = String::from("b");
    __see(&vec![&name].first());
    println!("{} {name}", view.name);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
