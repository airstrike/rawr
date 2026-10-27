fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let __t = &__id(&a);
    __see(__t);
    refs.push(&a);
    a += 1;
    println!("{refs:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
