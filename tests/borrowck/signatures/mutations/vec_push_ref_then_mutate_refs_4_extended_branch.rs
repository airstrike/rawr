fn main() {
    let mut a = 1;
    let mut refs = Vec::new();
    let __t = if __c() { &__id(&refs) } else { &__id(&refs) };
    refs.push(&a);
    __see(__t);
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
