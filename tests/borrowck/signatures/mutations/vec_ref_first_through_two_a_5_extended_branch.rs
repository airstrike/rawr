fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let f = refs.first();
    let __t = if __c() { &__id(&a) } else { &__id(&a) };
    a += 1;
    __see(__t);
    println!("{f:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
