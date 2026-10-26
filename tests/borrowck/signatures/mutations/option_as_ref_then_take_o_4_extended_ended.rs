fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    let __t = &__id(&o);
    __see(__t);
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
