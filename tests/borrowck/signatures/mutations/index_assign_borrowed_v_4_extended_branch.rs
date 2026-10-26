fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let __t = if __c() { &__id(&v) } else { &__id(&v) };
    v[0] = 5;
    __see(__t);
    println!("{}", r[0]);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
