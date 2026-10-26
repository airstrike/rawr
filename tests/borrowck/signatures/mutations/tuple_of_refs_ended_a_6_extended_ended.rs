fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    println!("{} {}", pair.0, pair.1);
    let __t = &__id(&a);
    __see(__t);
    a += 1;
    println!("{a}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
