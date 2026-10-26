fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __t = &__id(&v);
    v[0] = 5;
    __see(__t);
    println!("{} {:?}", first, v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
