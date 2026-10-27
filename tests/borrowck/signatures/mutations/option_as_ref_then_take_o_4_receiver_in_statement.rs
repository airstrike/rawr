fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    __see(&vec![&o].first());
    o.take();
    println!("{r:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
