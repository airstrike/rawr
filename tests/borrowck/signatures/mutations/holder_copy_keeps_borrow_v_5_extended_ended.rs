fn main() {
    let mut v = vec![1];
    let r = &v;
    let s = r;
    let __t = &__id(&v);
    __see(__t);
    v.push(2);
    println!("{}", s.len());
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
