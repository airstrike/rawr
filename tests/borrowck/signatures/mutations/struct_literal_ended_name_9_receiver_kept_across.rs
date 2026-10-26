struct View<'a> {
    name: &'a String,
}

fn main() {
    let mut name = String::from("a");
    let view = View { name: &name };
    println!("{}", view.name);
    let __t = vec![&name].first();
    name = String::from("b");
    __see(&__t);
    println!("{name}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
