fn main() {
    let mut a = 1;
    let b = 2;
    let pair = (&a, &b);
    let __t = &__id(&a);
    a += 1;
    __see(__t);
    println!("{}", pair.1);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
