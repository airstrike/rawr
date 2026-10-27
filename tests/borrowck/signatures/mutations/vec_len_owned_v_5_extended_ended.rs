fn main() {
    let mut v = vec![1, 2];
    let n = v.len();
    v.push(3);
    let __t = &__id(&v);
    __see(__t);
    println!("{n} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
