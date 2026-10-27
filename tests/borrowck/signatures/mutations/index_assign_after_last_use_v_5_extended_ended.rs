fn main() {
    let mut v = vec![1, 2];
    let r = &v;
    let first = r[0];
    let __t = &__id(&v);
    __see(__t);
    v[0] = 5;
    println!("{} {:?}", first, v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
