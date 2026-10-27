fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    let __t = &__id(&v);
    __see(__t);
    v.push(3);
    println!("{w:?} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
