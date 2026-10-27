fn main() {
    let mut x = 1;
    let r = &x;
    let __t = if __c() { &__id(&x) } else { &__id(&x) };
    x = 2;
    __see(__t);
    println!("{}", *r + x);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
