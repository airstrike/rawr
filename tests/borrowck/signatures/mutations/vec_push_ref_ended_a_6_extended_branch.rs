fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    refs.push(&a);
    println!("{refs:?}");
    let __t = if __c() { &__id(&a) } else { &__id(&a) };
    a += 1;
    __see(__t);
    println!("{a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
