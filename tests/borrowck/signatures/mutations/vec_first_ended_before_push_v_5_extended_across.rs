fn main() {
    let mut v = vec![1, 2, 3];
    let f = v.first();
    println!("{f:?}");
    let __t = &__id(&v);
    v.push(4);
    __see(__t);
    println!("{v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
