fn main() {
    let mut v = vec![1, 2];
    v.push(v.len());
    let r = &v;
    let __t = &__id(&v);
    __see(__t);
    v.push(r.len());
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
