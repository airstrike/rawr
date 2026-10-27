fn main() {
    let mut v = vec![1, 2];
    let __t = if __c() { &__id(&v) } else { &__id(&v) };
    v.push(v.len());
    __see(__t);
    let r = &v;
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
