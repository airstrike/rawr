fn main() {
    let mut v = vec![1, 2];
    let __t = &__id(&v);
    v[v.len() - 1] = 5;
    __see(__t);
    println!("{:?}", v);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
