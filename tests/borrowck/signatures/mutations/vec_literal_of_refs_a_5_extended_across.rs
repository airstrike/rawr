fn main() {
    let mut a = 1;
    let b = 2;
    let refs = vec![&a, &b];
    let __t = &__id(&a);
    a = 5;
    __see(__t);
    println!("{refs:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
