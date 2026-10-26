fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    let __t = &__id(&x);
    __see(__t);
    x = 2;
    println!("{}", x + y);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
