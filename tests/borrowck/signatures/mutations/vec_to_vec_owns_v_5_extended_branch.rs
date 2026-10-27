fn main() {
    let mut v = vec![1, 2];
    let w = v.to_vec();
    v.push(3);
    let __t = if __c() { &__id(&v) } else { &__id(&v) };
    println!("{w:?} {v:?}");
    __see(__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
