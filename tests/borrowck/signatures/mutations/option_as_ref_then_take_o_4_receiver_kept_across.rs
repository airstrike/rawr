fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    let __t = vec![&o].first();
    o.take();
    __see(&__t);
    println!("{r:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
