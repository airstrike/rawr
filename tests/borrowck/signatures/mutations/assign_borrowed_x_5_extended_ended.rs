fn main() {
    let mut x = 1;
    let r = &x;
    x = 2;
    let __t = &__id(&x);
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
