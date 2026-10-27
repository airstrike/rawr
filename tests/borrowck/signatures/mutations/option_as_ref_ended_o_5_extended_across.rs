fn main() {
    let mut o = Some(String::from("x"));
    let r = o.as_ref();
    println!("{r:?}");
    let __t = &__id(&o);
    o.take();
    __see(__t);
    println!("{o:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
