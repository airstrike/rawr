struct View<'a> {
    name: &'a String,
}

fn main() {
    let mut name = String::from("a");
    let view = View { name: &name };
    let __t = if __c() { &__id(&name) } else { &__id(&name) };
    println!("{}", view.name);
    __see(__t);
    name = String::from("b");
    println!("{name}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
